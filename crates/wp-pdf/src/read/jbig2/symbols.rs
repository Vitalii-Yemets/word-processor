//! Symbol dictionaries and the text regions that draw with them.
//!
//! [T.88] 6.4 and 6.5. A page of text is mostly the same few shapes over
//! and over: a symbol dictionary codes each shape once — by height class,
//! each symbol a width more than the one before, its bitmap coded as a
//! generic region, as a refinement of one before, or as several before put
//! together — and a text region then codes where each copy goes, strip by
//! strip down the region and symbol by symbol along each, and which symbol
//! it is, perhaps refined to fit. Either may be coded with the MQ coder or
//! with Huffman tables.

use super::super::mq::{Context, Decoder};
use super::bitmap::{Bitmap, Combine};
use super::generic::{self, Generic, Refinement};
use super::huffman::{self, Bits, Table, Value};
use super::integers::{bits_for, Integers, SymbolIds};

/// Bitmaps bigger than this are not a symbol or a region.
const MOST_PIXELS: usize = 1 << 28;

/// The integer decoders of a text region.
pub struct TextIntegers {
    dt: Integers,
    fs: Integers,
    ds: Integers,
    it: Integers,
    ri: Integers,
    rdw: Integers,
    rdh: Integers,
    rdx: Integers,
    rdy: Integers,
    id: SymbolIds,
}

impl TextIntegers {
    #[must_use]
    pub fn new(code_length: u32) -> Self {
        Self {
            dt: Integers::default(),
            fs: Integers::default(),
            ds: Integers::default(),
            it: Integers::default(),
            ri: Integers::default(),
            rdw: Integers::default(),
            rdh: Integers::default(),
            rdx: Integers::default(),
            rdy: Integers::default(),
            id: SymbolIds::new(code_length),
        }
    }
}

/// The Huffman tables of a text region.
pub struct TextTables {
    pub fs: Table,
    pub ds: Table,
    pub dt: Table,
    pub rdw: Table,
    pub rdh: Table,
    pub rdx: Table,
    pub rdy: Table,
    pub rsize: Table,
    /// The symbols' codes, or none where they are plain numbers so many
    /// bits long.
    pub ids: Option<Table>,
}

impl TextTables {
    /// The tables a symbol dictionary's aggregates use.
    #[must_use]
    pub fn for_aggregates() -> Self {
        Self {
            fs: huffman::standard(6),
            ds: huffman::standard(8),
            dt: huffman::standard(11),
            rdw: huffman::standard(15),
            rdh: huffman::standard(15),
            rdx: huffman::standard(15),
            rdy: huffman::standard(15),
            rsize: huffman::standard(1),
            ids: None,
        }
    }
}

/// How a text region's numbers are coded.
pub enum Coding<'a, 'b> {
    Arithmetic { decoder: &'b mut Decoder<'a>, integers: &'b mut TextIntegers },
    Huffman { bits: &'b mut Bits<'a>, tables: &'b TextTables },
}

#[derive(Clone, Copy)]
enum Number {
    Dt,
    Fs,
    Ds,
    It,
    Ri,
    Rdw,
    Rdh,
    Rdx,
    Rdy,
}

impl Coding<'_, '_> {
    fn number(&mut self, which: Number, log_strips: u32) -> Option<Value> {
        match self {
            Coding::Arithmetic { decoder, integers } => {
                let integers = &mut **integers;
                let chosen = match which {
                    Number::Dt => &mut integers.dt,
                    Number::Fs => &mut integers.fs,
                    Number::Ds => &mut integers.ds,
                    Number::It => &mut integers.it,
                    Number::Ri => &mut integers.ri,
                    Number::Rdw => &mut integers.rdw,
                    Number::Rdh => &mut integers.rdh,
                    Number::Rdx => &mut integers.rdx,
                    Number::Rdy => &mut integers.rdy,
                };
                Some(chosen.decode(decoder))
            }
            Coding::Huffman { bits, tables } => match which {
                Number::It => Some(Some(i64::from(bits.read(log_strips)?))),
                Number::Ri => Some(Some(i64::from(bits.bit()?))),
                Number::Dt => tables.dt.decode(bits),
                Number::Fs => tables.fs.decode(bits),
                Number::Ds => tables.ds.decode(bits),
                Number::Rdw => tables.rdw.decode(bits),
                Number::Rdh => tables.rdh.decode(bits),
                Number::Rdx => tables.rdx.decode(bits),
                Number::Rdy => tables.rdy.decode(bits),
            },
        }
    }

    fn symbol(&mut self, code_length: u32) -> Option<usize> {
        match self {
            Coding::Arithmetic { decoder, integers } => Some(integers.id.decode(decoder)),
            Coding::Huffman { bits, tables } => match &tables.ids {
                Some(table) => usize::try_from(table.decode(bits)??).ok(),
                None => Some(bits.read(code_length)? as usize),
            },
        }
    }
}

/// What a text region is.
pub struct Text<'s> {
    pub refine: bool,
    pub width: usize,
    pub height: usize,
    pub instances: usize,
    pub log_strips: u32,
    pub symbols: &'s [Bitmap],
    pub code_length: u32,
    pub default_pixel: u8,
    pub combine: Combine,
    pub transposed: bool,
    /// Which corner of a symbol its place is: bottom left, top left,
    /// bottom right or top right.
    pub corner: u8,
    pub offset: i64,
    pub refinement: Refinement,
}

const BOTTOM_LEFT: u8 = 0;
const TOP_LEFT: u8 = 1;
const BOTTOM_RIGHT: u8 = 2;
const TOP_RIGHT: u8 = 3;

/// Decodes a text region. `data` is the whole of what `coding` reads,
/// for the refinements a Huffman-coded region codes arithmetically.
pub fn decode_text(
    text: &Text<'_>,
    coding: &mut Coding<'_, '_>,
    data: &[u8],
    refinement_contexts: &mut [Context],
) -> Option<Bitmap> {
    if text.width * text.height > MOST_PIXELS {
        return None;
    }
    let mut region = Bitmap::filled(text.width, text.height, text.default_pixel);
    let strips = 1i64 << text.log_strips;
    let mut strip_t = -(coding.number(Number::Dt, text.log_strips)?? * strips);
    let mut first_s = 0i64;
    let mut instances = 0usize;
    while instances < text.instances {
        strip_t += coding.number(Number::Dt, text.log_strips)?? * strips;
        let mut first = true;
        let mut current_s = 0i64;
        loop {
            if first {
                first_s += coding.number(Number::Fs, text.log_strips)??;
                current_s = first_s;
                first = false;
            } else {
                if instances > text.instances + 1 {
                    return Some(region);
                }
                match coding.number(Number::Ds, text.log_strips)? {
                    Some(step) => current_s += step + text.offset,
                    None => break,
                }
            }
            let current_t =
                if strips == 1 { 0 } else { coding.number(Number::It, text.log_strips)?? };
            let t = strip_t + current_t;
            let id = coding.symbol(text.code_length)?;
            let refined = text.refine && coding.number(Number::Ri, text.log_strips)?? != 0;
            let symbol = text.symbols.get(id)?;
            let refined_bitmap;
            let bitmap = if refined {
                refined_bitmap = refine_symbol(text, coding, data, symbol, refinement_contexts)?;
                &refined_bitmap
            } else {
                symbol
            };
            let (w, h) = (bitmap.width as i64, bitmap.height as i64);
            if !text.transposed && matches!(text.corner, TOP_RIGHT | BOTTOM_RIGHT) {
                current_s += w - 1;
            }
            if text.transposed && matches!(text.corner, BOTTOM_LEFT | BOTTOM_RIGHT) {
                current_s += h - 1;
            }
            let s = current_s;
            let (x, y) = match (text.transposed, text.corner) {
                (false, TOP_LEFT) => (s, t),
                (false, TOP_RIGHT) => (s - w + 1, t),
                (false, BOTTOM_LEFT) => (s, t - h + 1),
                (false, _) => (s - w + 1, t - h + 1),
                (true, TOP_LEFT) => (t, s),
                (true, TOP_RIGHT) => (t - w + 1, s),
                (true, BOTTOM_LEFT) => (t, s - h + 1),
                (true, _) => (t - w + 1, s - h + 1),
            };
            region.compose(bitmap, x, y, text.combine);
            if !text.transposed && matches!(text.corner, TOP_LEFT | BOTTOM_LEFT) {
                current_s += w - 1;
            }
            if text.transposed && matches!(text.corner, TOP_LEFT | TOP_RIGHT) {
                current_s += h - 1;
            }
            instances += 1;
        }
    }
    Some(region)
}

/// A symbol refined where a text region places it.
fn refine_symbol(
    text: &Text<'_>,
    coding: &mut Coding<'_, '_>,
    data: &[u8],
    symbol: &Bitmap,
    contexts: &mut [Context],
) -> Option<Bitmap> {
    let rdw = coding.number(Number::Rdw, text.log_strips)??;
    let rdh = coding.number(Number::Rdh, text.log_strips)??;
    let rdx = coding.number(Number::Rdx, text.log_strips)??;
    let rdy = coding.number(Number::Rdy, text.log_strips)??;
    let width = usize::try_from(symbol.width as i64 + rdw).ok()?;
    let height = usize::try_from(symbol.height as i64 + rdh).ok()?;
    if width * height > MOST_PIXELS {
        return None;
    }
    let parameters = Refinement {
        dx: rdw.div_euclid(2) + rdx,
        dy: rdh.div_euclid(2) + rdy,
        typical: false,
        ..text.refinement
    };
    match coding {
        Coding::Arithmetic { decoder, .. } => {
            Some(generic::decode_refinement(decoder, contexts, width, height, symbol, &parameters))
        }
        Coding::Huffman { bits, tables } => {
            let size = usize::try_from(tables.rsize.decode(bits)??).ok()?;
            bits.align();
            let start = bits.byte();
            let end = start.checked_add(size)?.min(data.len());
            let mut decoder = Decoder::new(data, start, end);
            let bitmap = generic::decode_refinement(
                &mut decoder,
                contexts,
                width,
                height,
                symbol,
                &parameters,
            );
            bits.at = (start + size) * 8;
            Some(bitmap)
        }
    }
}

/// The symbols' code table a Huffman-coded text region sends first,
/// [T.88] 7.4.3.1.7: the lengths of the codes of the code lengths, then
/// the code lengths, some of them repeated.
pub fn symbol_code_table(bits: &mut Bits<'_>, count: usize) -> Option<Table> {
    let mut run_lengths = [0u32; 35];
    for slot in &mut run_lengths {
        *slot = bits.read(4)?;
    }
    let runs = Table::of_lengths(&run_lengths);
    let mut lengths: Vec<u32> = Vec::with_capacity(count);
    while lengths.len() < count {
        let code = runs.decode(bits)??;
        match code {
            0..=31 => lengths.push(code as u32),
            32 => {
                let previous = *lengths.last()?;
                let times = 3 + bits.read(2)?;
                lengths.extend(std::iter::repeat_n(previous, times as usize));
            }
            33 => {
                let times = 3 + bits.read(3)?;
                lengths.extend(std::iter::repeat_n(0, times as usize));
            }
            _ => {
                let times = 11 + bits.read(7)?;
                lengths.extend(std::iter::repeat_n(0, times as usize));
            }
        }
    }
    lengths.truncate(count);
    bits.align();
    Some(Table::of_lengths(&lengths))
}

/// What a symbol dictionary is.
pub struct DictionaryParameters {
    pub huffman: bool,
    pub aggregate: bool,
    pub generic: Generic,
    pub refinement: Refinement,
    pub exported: usize,
    pub new: usize,
    pub dh: Table,
    pub dw: Table,
    pub size: Table,
    pub instances: Table,
}

/// A symbol dictionary decoded: the symbols it offers, and the contexts
/// it leaves for one after to carry on with.
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    pub symbols: Vec<Bitmap>,
    pub generic: Vec<Context>,
    pub refinement: Vec<Context>,
}

/// Decodes a symbol dictionary, given the symbols of the dictionaries it
/// refers to, and the contexts one of them left if it is to carry on.
pub fn decode_dictionary(
    parameters: &DictionaryParameters,
    input: &[Bitmap],
    data: &[u8],
    carried: Option<(Vec<Context>, Vec<Context>)>,
) -> Option<Dictionary> {
    let (mut generic_contexts, mut refinement_contexts) = carried.unwrap_or_else(|| {
        (
            generic::generic_contexts(parameters.generic.template),
            generic::refinement_contexts(parameters.refinement.template),
        )
    });
    let code_length = bits_for(input.len() + parameters.new);
    let mut decoder = Decoder::new(data, 0, data.len());
    let mut bits = Bits { data, at: 0 };
    let mut text_integers = TextIntegers::new(code_length);
    let (mut dh, mut dw, mut ex, mut ai) =
        (Integers::default(), Integers::default(), Integers::default(), Integers::default());
    let aggregate_tables = TextTables::for_aggregates();
    let mut new: Vec<Bitmap> = Vec::with_capacity(parameters.new.min(1 << 16));
    let mut height_class = 0i64;
    while new.len() < parameters.new {
        let step = if parameters.huffman {
            parameters.dh.decode(&mut bits)??
        } else {
            dh.decode(&mut decoder)?
        };
        height_class += step;
        let height = usize::try_from(height_class).ok()?;
        let mut symbol_width = 0i64;
        let mut total_width = 0usize;
        let mut widths: Vec<usize> = Vec::new();
        loop {
            let step = if parameters.huffman {
                parameters.dw.decode(&mut bits)?
            } else {
                dw.decode(&mut decoder)
            };
            let Some(step) = step else { break };
            if new.len() + widths.len() >= parameters.new {
                return None;
            }
            symbol_width += step;
            let width = usize::try_from(symbol_width).ok()?;
            total_width += width;
            if width * height > MOST_PIXELS {
                return None;
            }
            if parameters.huffman && !parameters.aggregate {
                widths.push(width);
                continue;
            }
            let bitmap = if !parameters.aggregate {
                generic::decode_generic(
                    &mut decoder,
                    &mut generic_contexts,
                    width,
                    height,
                    &parameters.generic,
                )
            } else {
                let count = if parameters.huffman {
                    parameters.instances.decode(&mut bits)??
                } else {
                    ai.decode(&mut decoder)?
                };
                let symbols: Vec<Bitmap> = input.iter().chain(&new).cloned().collect();
                if count > 1 {
                    let text = Text {
                        refine: true,
                        width,
                        height,
                        instances: usize::try_from(count).ok()?,
                        log_strips: 0,
                        symbols: &symbols,
                        code_length,
                        default_pixel: 0,
                        combine: Combine::Or,
                        transposed: false,
                        corner: TOP_LEFT,
                        offset: 0,
                        refinement: parameters.refinement,
                    };
                    let mut coding = if parameters.huffman {
                        Coding::Huffman { bits: &mut bits, tables: &aggregate_tables }
                    } else {
                        Coding::Arithmetic { decoder: &mut decoder, integers: &mut text_integers }
                    };
                    decode_text(&text, &mut coding, data, &mut refinement_contexts)?
                } else {
                    let (id, rdx, rdy) = if parameters.huffman {
                        let id = bits.read(code_length)? as usize;
                        let rdx = aggregate_tables.rdx.decode(&mut bits)??;
                        let rdy = aggregate_tables.rdy.decode(&mut bits)??;
                        (id, rdx, rdy)
                    } else {
                        let id = text_integers.id.decode(&mut decoder);
                        let rdx = text_integers.rdx.decode(&mut decoder)?;
                        let rdy = text_integers.rdy.decode(&mut decoder)?;
                        (id, rdx, rdy)
                    };
                    let reference = symbols.get(id)?;
                    let refinement =
                        Refinement { dx: rdx, dy: rdy, typical: false, ..parameters.refinement };
                    if parameters.huffman {
                        let size =
                            usize::try_from(huffman::standard(1).decode(&mut bits)??).ok()?;
                        bits.align();
                        let start = bits.byte();
                        let end = start.checked_add(size)?.min(data.len());
                        let mut inner = Decoder::new(data, start, end);
                        let bitmap = generic::decode_refinement(
                            &mut inner,
                            &mut refinement_contexts,
                            width,
                            height,
                            reference,
                            &refinement,
                        );
                        bits.at = (start + size) * 8;
                        bitmap
                    } else {
                        generic::decode_refinement(
                            &mut decoder,
                            &mut refinement_contexts,
                            width,
                            height,
                            reference,
                            &refinement,
                        )
                    }
                }
            };
            new.push(bitmap);
        }
        if parameters.huffman && !parameters.aggregate {
            // The height class's symbols side by side in one bitmap.
            let size = usize::try_from(parameters.size.decode(&mut bits)??).ok()?;
            bits.align();
            let start = bits.byte();
            if total_width * height > MOST_PIXELS {
                return None;
            }
            let collective = if size == 0 {
                let length = total_width.div_ceil(8) * height;
                let end = start.checked_add(length)?.min(data.len());
                bits.at = (start + length) * 8;
                Bitmap::from_packed(&data[start.min(end)..end], total_width, height)
            } else {
                let end = start.checked_add(size)?.min(data.len());
                bits.at = (start + size) * 8;
                generic::decode_mmr(&data[start.min(end)..end], total_width, height)?.0
            };
            let mut x = 0i64;
            for width in widths {
                new.push(collective.part(x, 0, width, height));
                x += width as i64;
            }
        }
    }
    // Which of the symbols, old and new, the dictionary offers: runs of
    // them, alternately not and so.
    let total = input.len() + new.len();
    let mut exported = Vec::with_capacity(parameters.exported.min(total));
    let mut offered = false;
    let mut index = 0usize;
    while index < total {
        let run = if parameters.huffman {
            huffman::standard(1).decode(&mut bits)??
        } else {
            ex.decode(&mut decoder)?
        };
        let run = usize::try_from(run).ok()?;
        if run > total - index {
            return None;
        }
        if offered {
            for at in index..index + run {
                exported.push(if at < input.len() {
                    input[at].clone()
                } else {
                    new[at - input.len()].clone()
                });
            }
        }
        index += run;
        offered = !offered;
    }
    Some(Dictionary {
        symbols: exported,
        generic: generic_contexts,
        refinement: refinement_contexts,
    })
}

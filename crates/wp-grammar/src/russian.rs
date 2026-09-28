//! The rules for Russian.
//!
//! Each is a mistake only in the company it is written with. "В течении" is
//! right about a river and wrong about a year, "одеть" is right about a child
//! and wrong about a coat, "по окончанию" is right in "работы по окончанию
//! строительства" — so each rule names the words around the mistake that make
//! it one, and says nothing where it cannot tell.

use crate::{Piece, Rule};

/// The words of time "в течение" goes before: "в течение года", "в течение
/// всего дня". Only these, because "в течении реки" is the river's current
/// and right.
const SPANS_OF_TIME: &[&str] = &[
    "секунды",
    "минуты",
    "минут",
    "часа",
    "часов",
    "получаса",
    "дня",
    "дней",
    "суток",
    "ночи",
    "утра",
    "вечера",
    "недели",
    "недель",
    "месяца",
    "месяцев",
    "квартала",
    "полугода",
    "года",
    "лет",
    "семестра",
    "сезона",
    "лета",
    "зимы",
    "весны",
    "осени",
    "века",
    "десятилетия",
    "столетия",
    "времени",
    "периода",
    "срока",
    "жизни",
    "всего",
    "всей",
    "нескольких",
    "многих",
    "двух",
    "трёх",
    "трех",
    "последних",
    "ближайших",
    "первых",
    "долгого",
    "долгих",
];

/// The comparatives that "более" and "менее" cannot go before, being
/// comparatives already.
const COMPARATIVES: &[&str] = &[
    "лучше",
    "хуже",
    "больше",
    "меньше",
    "выше",
    "ниже",
    "старше",
    "младше",
    "дороже",
    "дешевле",
    "сильнее",
    "слабее",
    "быстрее",
    "медленнее",
    "легче",
    "тяжелее",
    "проще",
    "сложнее",
    "важнее",
    "интереснее",
    "красивее",
    "лучший",
    "лучшая",
    "лучшее",
    "лучшие",
    "лучшего",
    "лучшей",
    "лучших",
    "худший",
    "худшая",
    "худшее",
    "худшие",
];

/// "Одеть" is what is done to somebody, "надеть" what is done with a
/// garment: the forms of the one and what the other says in their place.
const ODET: &[(&str, &str)] = &[
    ("одеть", "надеть"),
    ("одел", "надел"),
    ("одела", "надела"),
    ("одели", "надели"),
    ("одену", "надену"),
    ("оденешь", "наденешь"),
    ("оденет", "наденет"),
    ("оденем", "наденем"),
    ("оденете", "наденете"),
    ("оденут", "наденут"),
    ("одень", "надень"),
    ("оденьте", "наденьте"),
    ("одевать", "надевать"),
    ("одеваю", "надеваю"),
    ("одеваешь", "надеваешь"),
    ("одевает", "надевает"),
    ("одеваем", "надеваем"),
    ("одеваете", "надеваете"),
    ("одевают", "надевают"),
    ("одевал", "надевал"),
    ("одевала", "надевала"),
    ("одевали", "надевали"),
];

/// Garments, as the thing put on: what makes "одел" a mistake rather than
/// the right word for dressing a child.
const GARMENTS: &[&str] = &[
    "пальто",
    "куртку",
    "плащ",
    "шубу",
    "шапку",
    "шляпу",
    "шарф",
    "платье",
    "юбку",
    "брюки",
    "джинсы",
    "рубашку",
    "футболку",
    "майку",
    "кофту",
    "блузку",
    "свитер",
    "пиджак",
    "жилет",
    "костюм",
    "халат",
    "фартук",
    "пижаму",
    "форму",
    "перчатки",
    "варежки",
    "носки",
    "туфли",
    "ботинки",
    "сапоги",
    "кроссовки",
    "очки",
    "маску",
    "кольцо",
    "серьги",
    "часы",
    "галстук",
    "ремень",
    "шлем",
];

/// "Ложить" is not a verb of the written language; "класть" is, and these
/// are the forms of the one and of the other. Not "ложу" or "ложи", which are
/// also the box at a theatre.
const LOZHIT: &[(&str, &str)] = &[
    ("ложить", "класть"),
    ("ложил", "клал"),
    ("ложила", "клала"),
    ("ложило", "клало"),
    ("ложили", "клали"),
    ("ложишь", "кладёшь"),
    ("ложит", "кладёт"),
    ("ложим", "кладём"),
    ("ложите", "кладёте"),
    ("ложат", "кладут"),
];

/// "По" meaning "after" takes the prepositional case: "по приезде", not "по
/// приезду". Only the nouns that "по" hardly ever governs otherwise — not
/// "окончанию", which is right in "работы по окончанию".
const PO_AFTER: &[(&str, &str)] = &[
    ("приезду", "приезде"),
    ("прибытию", "прибытии"),
    ("прилёту", "прилёте"),
    ("прилету", "прилете"),
    ("истечению", "истечении"),
];

/// "Согласно" takes the dative: "согласно приказу", not "согласно приказа".
const SOGLASNO: &[(&str, &str)] = &[
    ("приказа", "приказу"),
    ("закона", "закону"),
    ("договора", "договору"),
    ("плана", "плану"),
    ("графика", "графику"),
    ("расписания", "расписанию"),
    ("решения", "решению"),
    ("распоряжения", "распоряжению"),
    ("указания", "указанию"),
    ("указаний", "указаниям"),
    ("требования", "требованию"),
    ("требований", "требованиям"),
    ("правил", "правилам"),
    ("постановления", "постановлению"),
    ("пункта", "пункту"),
    ("статьи", "статье"),
    ("устава", "уставу"),
    ("условий", "условиям"),
    ("данных", "данным"),
    ("инструкций", "инструкциям"),
];

/// "Оба" for men and things, "обе" for women and the feminine nouns: the
/// forms of the one and of the other.
const OBOIH: &[(&str, &str)] = &[("обоих", "обеих"), ("обоим", "обеим"), ("обоими", "обеими")];

/// Feminine nouns in the plural, which want "обеих" and not "обоих".
const FEMININE_PLURALS: &[&str] = &[
    "сторон",
    "сторонам",
    "сторонами",
    "сторонах",
    "рук",
    "рукам",
    "руками",
    "руках",
    "ног",
    "ногам",
    "ногами",
    "ногах",
    "девушек",
    "девушкам",
    "женщин",
    "женщинам",
    "сестёр",
    "сестер",
    "сёстрам",
    "сестрам",
    "команд",
    "командам",
    "стран",
    "странам",
    "частей",
    "частям",
    "книг",
    "машин",
    "дочерей",
];

pub(crate) const RULES: &[Rule] = &[
    // --- Commonly confused words -------------------------------------------
    Rule {
        pattern: &[Piece::Word("в"), Piece::Word("течении"), Piece::AnyOf(SPANS_OF_TIME)],
        category: "Commonly confused words",
        replacement: Some("$1 течение $3"),
        example: "Он работал в течении года.",
    },
    Rule {
        pattern: &[Piece::Word("в"), Piece::Word("продолжении"), Piece::AnyOf(SPANS_OF_TIME)],
        category: "Commonly confused words",
        replacement: Some("$1 продолжение $3"),
        example: "Мы молчали в продолжении часа.",
    },
    Rule {
        pattern: &[
            Piece::Word("в"),
            Piece::Word("следствие"),
            Piece::AnyOf(&["чего", "этого", "того", "сего"]),
        ],
        category: "Commonly confused words",
        replacement: Some("вследствие $3"),
        example: "В следствие этого поезд опоздал.",
    },
    Rule {
        pattern: &[Piece::Swap(ODET), Piece::AnyOf(GARMENTS)],
        category: "Commonly confused words",
        replacement: Some("%1 $2"),
        example: "Он одел пальто и вышел.",
    },
    Rule {
        pattern: &[Piece::Word("никто"), Piece::Word("иной"), Piece::Word("как")],
        category: "Commonly confused words",
        replacement: Some("не кто иной, как"),
        example: "Это был никто иной как директор.",
    },
    Rule {
        pattern: &[Piece::Word("ничто"), Piece::Word("иное"), Piece::Word("как")],
        category: "Commonly confused words",
        replacement: Some("не что иное, как"),
        example: "Это ничто иное как ошибка.",
    },
    // --- Word form ---------------------------------------------------------
    Rule {
        pattern: &[Piece::Word("по"), Piece::Swap(PO_AFTER)],
        category: "Word form",
        replacement: Some("$1 %2"),
        example: "Позвоните по приезду домой.",
    },
    Rule {
        pattern: &[Piece::Word("согласно"), Piece::Swap(SOGLASNO)],
        category: "Word form",
        replacement: Some("$1 %2"),
        example: "Работы ведутся согласно графика.",
    },
    Rule {
        pattern: &[Piece::Swap(OBOIH), Piece::AnyOf(FEMININE_PLURALS)],
        category: "Word form",
        replacement: Some("%1 $2"),
        example: "Он держал руль обоими руками.",
    },
    // --- Verb form ---------------------------------------------------------
    Rule {
        pattern: &[Piece::Swap(LOZHIT)],
        category: "Verb form",
        replacement: Some("%1"),
        example: "Он ложил книги на стол.",
    },
    Rule {
        pattern: &[Piece::Swap(&[("ехай", "поезжай"), ("ехайте", "поезжайте")])],
        category: "Verb form",
        replacement: Some("%1"),
        example: "Ехай осторожно.",
    },
    // --- Word choice -------------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(&[
            "ихний",
            "ихняя",
            "ихнее",
            "ихние",
            "ихнего",
            "ихней",
            "ихнему",
            "ихним",
            "ихних",
            "ихними",
            "ихнюю",
        ])],
        category: "Word choice",
        replacement: Some("их"),
        example: "Это ихний дом.",
    },
    // --- Comparisons -------------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(&["более", "менее"]), Piece::AnyOf(COMPARATIVES)],
        category: "Comparisons",
        replacement: Some("$2"),
        example: "Этот вариант более лучше.",
    },
];

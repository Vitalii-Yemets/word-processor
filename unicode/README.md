# The Unicode conformance suites

The Consortium publishes test files that say what a conforming implementation
must answer. Put them here and the text engine is held to them:

```powershell
.\x.ps1 conformance    # or ./x.sh conformance
```

## The six files

From `https://www.unicode.org/Public/<version>/ucd/`:

- `BidiTest.txt` — the bidirectional algorithm over sequences of classes
- `BidiCharacterTest.txt` — the same over real text
- `NormalizationTest.txt` — NFC and NFD (the NFKC and NFKD columns are read
  past: compatibility normalization changes what the text says, and this
  program does not do it)

And from `.../ucd/auxiliary/`:

- `GraphemeBreakTest.txt` — where one character ends and the next begins
- `WordBreakTest.txt` — where a word ends
- `LineBreakTest.txt` — where a line may be broken

**Take the version the tables were generated from**, which is the version of
the character database in the build image — Unicode 14.0.0 at the time of
writing, and `tools/generate-unicode-tables.sh` is what produced them. Holding
tables built from one version to another version's answers measures the gap
between the two and calls it a bug.

## Why they are not committed

Nothing here is downloaded or installed by the build; that is the rule the
whole project is built under. The files come from whoever wants to run them,
and this directory is ignored by git apart from this README. A suite whose
file is not here is reported missing — not passed, and not failed.

## What the command reports

How many cases each suite ran, how many passed, and the first few that did
not, with the line each is on. The totals go into `conformance.log` so they
can be seen to move, and the report says which way they went since the run
before.

It reports rather than gates. Line breaking here keeps seventeen classes where
the standard has about forty, which is written down in the roadmap as **E16**;
a red build every morning would only say it again.

undef $/;
$_ = <STDIN>;
my $what = $ARGV[0];

if ($what eq 'mod') {
  s~mod protection;~mod protection;\nmod readonly;~ or die "mod";
  s~    /// A file that turned out to be encrypted, waiting for its password\.\n    waiting_to_unseal: Option<sealing::Waiting>,~    /// A file that turned out to be encrypted, waiting for its password.
    waiting_to_unseal: Option<sealing::Waiting>,
    /// Whether the document open now was opened read-only, because it asked
    /// to be. A property of this window and not of the document: see
    /// [`readonly`].
    opened_read_only: bool,~ or die "field";
  s~            waiting_to_unseal: None,~            waiting_to_unseal: None,
            opened_read_only: false,~ or die "init";
}

if ($what eq 'asking') {
  s~pub\(super\) enum Asking \{~pub(super) enum Asking {
    /// Whether a document that asks to be opened read-only is to be, and the
    /// password that opens it for writing.
    OpenReadOnly,
    /// What a document should ask for the next time it is opened.
    ReadOnlySettings,~ or die "variants";
  s~            Some\(Asking::Protect\) => self\.apply_protection\(&dialog\),~            Some(Asking::Protect) => self.apply_protection(&dialog),
            Some(Asking::OpenReadOnly) => self.apply_open_read_only(&dialog),
            Some(Asking::ReadOnlySettings) => self.apply_read_only_settings(&dialog),~ or die "accept";
  s~                Some\(Asking::Unseal\) => self\.cancel_unseal\(\),~                Some(Asking::Unseal) => self.cancel_unseal(),
                Some(Asking::OpenReadOnly) => self.cancel_open_read_only(),~ or die "cancel";
}

if ($what eq 'files') {
  s~        self\.carries_macros = self\.document\.has_macros\(\);~        self.carries_macros = self.document.has_macros();
        // Whatever is opened is opened for writing until it asks not to be,
        // which is asked at the door and not here: see [`super::readonly`].
        self.opened_read_only = false;~ or die "set_document";

  s~            if compatibility \{ " \[Compatibility Mode\]" \} else \{ "" \}\n        \);~            if compatibility { " [Compatibility Mode]" } else { "" }
        );
        // Word's caption says so too, and it is the one place a person looks
        // to find out why nothing they type is arriving.
        let wanted = if self.opened_read_only {
            wanted.replace(" — Word Processor", " (Read-Only) — Word Processor")
        } else {
            wanted
        };~ or die "title";

  s~            Ok\(document\) => \{\n                self\.set_document\(document, Some\(path\.clone\(\)\)\);\n                self\.status = crate::messages::with\("Opened \{0\}", &\[&path\.display\(\)\.to_string\(\)\]\);\n                self\.remember_recent\(&path\);~            Ok(document) => {
                self.set_document(document, Some(path.clone()));
                self.status = crate::messages::with("Opened {0}", &[&path.display().to_string()]);
                self.remember_recent(&path);
                // A document may ask not to be written, and the asking
                // happens at the door rather than at the first keystroke.
                if let Some(response) = self.asked_at_the_door(&path) {
                    return response;
                }~ or die "open_path";
}

if ($what eq 'protection') {
  s~    pub\(super\) fn is_locked\(&self\) -> bool \{\n        let Some\(mode\) = self\.document\.protection\(\) else \{ return false \};~    pub(super) fn is_locked(&self) -> bool {
        // A document opened read-only is locked everywhere, whatever else it
        // says: the question was answered at the door. See
        // [`super::readonly`].
        if self.is_read_only() {
            return true;
        }
        let Some(mode) = self.document.protection() else { return false };~ or die "is_locked";

  s~    pub\(super\) fn refuse_locked\(&mut self\) -> Response \{\n        let note = match self\.document\.protection\(\) \{~    pub(super) fn refuse_locked(&mut self) -> Response {
        if self.is_read_only() {
            return self.refuse_read_only();
        }
        let note = match self.document.protection() {~ or die "refuse_locked";
}

if ($what eq 'draw') {
  s~            restricted: self\.document\.protection\(\),~            // A document opened read-only greys out what a read-only
            // restriction would, because it is the same answer arrived at by
            // another road.
            restricted: self
                .document
                .protection()
                .or_else(|| self.is_read_only().then_some(wp_docx::protection::EditMode::ReadOnly)),~ or die "draw";
}

print;

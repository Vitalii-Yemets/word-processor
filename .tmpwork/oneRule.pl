undef $/; $_ = <STDIN>;
my $what = $ARGV[0];
if ($what eq 'protection') {
  s~    pub\(super\) fn refuse_restricted\(&mut self, command: Command\) -> Option<Response> \{\n        let restriction = self\.document\.protection\(\);~    pub(super) fn refuse_restricted(&mut self, command: Command) -> Option<Response> {
        let restriction = self.restriction_now();~ or die "use";
  s~    /// Whether the restriction stands in the way of a command, and what to~    /// What restriction stands over the document as things are.
    ///
    /// The document's own, or the read-only it asked for at the door and was
    /// given - see [`super::readonly`]. One rule, because the ribbon and the
    /// command both ask it and a button that looks pressable and does nothing
    /// is what that is for.
    #[must_use]
    pub(super) fn restriction_now(&self) -> Option<EditMode> {
        self.document
            .protection()
            .or_else(|| self.is_read_only().then_some(EditMode::ReadOnly))
    }

    /// Whether the restriction stands in the way of a command, and what to~ or die "add";
}
if ($what eq 'draw') {
  s~            // A document opened read-only greys out what a read-only\n            // restriction would, because it is the same answer arrived at by\n            // another road\.\n            restricted: self\n                \.document\n                \.protection\(\)\n                \.or_else\(\|\| self\.is_read_only\(\)\.then_some\(wp_docx::protection::EditMode::ReadOnly\)\),~            restricted: self.restriction_now(),~ or die "draw";
}
print;

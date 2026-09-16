undef $/; $_ = <STDIN>;
s~    /// Reads whichever set of attributes the element carries, newer first\.\n    fn read\(element: &Element\) -> Option<Self> \{~    /// Reads whichever set of attributes the element carries, newer first.
    ///
    /// Crate-visible because the format writes a password the same way
    /// wherever it writes one: see [`crate::readonly`].
    pub(crate) fn read(element: &Element) -> Option<Self> {~ or die "read";
s~    fn unwrite\(element: &mut Element\) \{~    pub(crate) fn unwrite(element: &mut Element) {~ or die "unwrite";
s~    fn write\(&self, element: &mut Element, prefix: Option<&str>\) \{~    pub(crate) fn write(&self, element: &mut Element, prefix: Option<&str>) {~ or die "write";
print;

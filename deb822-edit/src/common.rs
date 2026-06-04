#[inline]
pub(crate) fn is_indent(c: char) -> bool {
    // deb822(5) says that continuation lines
    // start with a space (U+0020) or a tab (U+0009).
    c == ' ' || c == '\t'
}

#[inline]
pub(crate) fn is_newline(c: char) -> bool {
    c == '\n' || c == '\r'
}

#[inline]
pub(crate) fn is_valid_key_char(c: char) -> bool {
    // deb822(5) says valid field characters are US-ASCII
    // characters excluding control characters, space and colon
    // (i.e. characters in the ranges U+0021 to U+0039 and U+003B to U+007E).
    // I.e. printable characters except for space and colon.
    c.is_ascii_graphic() && c != ':' && c != ' '
}

#[inline]
pub(crate) fn is_valid_initial_key_char(c: char) -> bool {
    c != '-' && is_valid_key_char(c)
}

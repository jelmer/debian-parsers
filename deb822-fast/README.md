Lossy parser for deb822 format.

This parser is lossy in the sense that it will discard whitespace and comments
in the input.

This parser is optimized for speed and memory usage. It provides two APIs:

- **Owned API (default)**: Returns owned `String` values. Easy to use, no lifetime management.
- **Borrowed API** (`borrowed` module): Returns borrowed string slices. Lower allocation overhead
  (avoids String allocations for field data, but still allocates Vec structures for paragraphs
  and fields). Requires lifetime management.

For editing purposes where you need to preserve formatting, whitespace and comments,
you may want to use a more feature-complete parser like ``deb822-edit``.

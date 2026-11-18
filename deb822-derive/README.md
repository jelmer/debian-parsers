This crate provides a basic proc-macro for converting a Deb822Paragraph
into a Rust struct and vice versa.

You probably want to use the ``deb822_edit`` crate instead,
with the ``derive`` feature enabled.

# Example

```rust
use deb822_edit::Deb822;

#[derive(Deb822)]
struct Foo {
    field1: String,
    field2: Option<String>,
}

let paragraph: deb822::Deb822Paragraph = "field1: value1\nfield2: value2".parse().unwrap();
let foo: Foo = paragraph.into();
```

# Field Formatting Attributes

The derive macros support field formatting attributes to control how fields are serialized:

## `single_line`

Forces the field value to be on a single line.

```rust
#[derive(ToDeb822)]
struct Package {
    #[deb822(field = "Package", single_line)]
    name: String,
}
```

## `multi_line`

Ensures continuation lines start with a space character, following deb822 format conventions.

```rust
#[derive(ToDeb822)]
struct Package {
    #[deb822(field = "Description", multi_line)]
    description: String,
}
```

## `folded`

Strips leading and trailing whitespace from each line and joins them with spaces,
implementing RFC 822 folding behavior.

```rust
#[derive(ToDeb822)]
struct Package {
    #[deb822(field = "Depends", folded)]
    depends: String,
}
```

use debian_control::edit::Control;
use debian_control::fields::Priority;

pub fn main() {
    let mut control = Control::new();
    let mut source = control.add_source("hello");
    source.set_section(Some("rust"));

    let mut binary = control.add_binary("hello");
    binary.set_architecture(Some("amd64"));
    binary.set_priority(Some(Priority::Optional));
    binary.set_description(Some("Hello, world!"));

    println!("{}", control);
}

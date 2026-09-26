pub fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

pub fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

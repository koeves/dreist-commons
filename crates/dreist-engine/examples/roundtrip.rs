fn main() {
    let p = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    let n = dreist_formats::Node::parse(&text).unwrap();
    std::fs::write(std::env::args().nth(2).unwrap(), n.to_xml()).unwrap();
}

fn main() {
    use base64::{engine::general_purpose, Engine as _};
    let d = soshal_content_core::compress::compress_json_dict(r#"{"a":"hello world hello world"}"#);
    let p = soshal_content_core::compress::compress_json(r#"{"a":"hello world hello world"}"#);
    println!("dict  first8: {:?}", &d[..8.min(d.len())]);
    println!("plain first8: {:?}", &p[..8.min(p.len())]);
    println!(
        "dict  decodes back: {}",
        soshal_content_core::compress::decompress_json_dict(&d)
            == r#"{"a":"hello world hello world"}"#
    );
    let raw = general_purpose::STANDARD.decode(&d).unwrap();
    println!("dict raw magic: {:02X?}", &raw[..6]);
    println!(
        "gate eNo? {}  eF4? {}  WkgB? {}",
        d.starts_with("eNo"),
        d.starts_with("eF4"),
        d.starts_with("WkgB")
    );
    println!(
        "plain gate eNo? {} eF4? {} WkgB? {}",
        p.starts_with("eNo"),
        p.starts_with("eF4"),
        p.starts_with("WkgB")
    );
}

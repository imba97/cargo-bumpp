//! The tests for the whole module: a metadata-shaped document, escapes and
//! unicode, numbers, and the inputs that must be rejected. They are their own
//! file because they exercise the value type and the parser together.

use super::*;

#[test]
fn parses_a_metadata_like_document() {
    let text = r#"{"packages":[{"name":"a","version":"0.1.0","dependencies":[{"name":"b","req":"^0.1.0","path":"C:\\x"}]}],
            "workspace_members":["path+file:///x#0.1.0"],"workspace_root":"C:/x","resolve":null}"#;
    let json = Json::parse(text).unwrap();
    assert_eq!(json.str_at("workspace_root"), Some("C:/x"));
    let packages = json.get("packages").unwrap().as_array().unwrap();
    assert_eq!(packages[0].str_at("name"), Some("a"));
    let dep = &packages[0].get("dependencies").unwrap().as_array().unwrap()[0];
    assert_eq!(dep.str_at("req"), Some("^0.1.0"));
    assert_eq!(dep.str_at("path"), Some("C:\\x"));
    assert_eq!(json.get("resolve").unwrap(), &Json::Null);
    assert_eq!(
        json.get("workspace_members")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn handles_escapes_and_unicode() {
    let json = Json::parse(r#"{"s":"a\u00e9b\u4e2d\ud83d\ude00\"\\\/\n\t"}"#).unwrap();
    assert_eq!(json.str_at("s"), Some("aéb中😀\"\\/\n\t"));
}

#[test]
fn rejects_bad_input() {
    assert!(Json::parse("").is_err());
    assert!(Json::parse("{").is_err());
    assert!(Json::parse(r#"{"a":1,}"#).is_err());
    assert!(Json::parse(r#"{"a" 1}"#).is_err());
    assert!(Json::parse(r#"{"a":"b"}"#).is_ok());
    assert!(Json::parse(r#"{"a":"b"} trailing"#).is_err());
    assert!(Json::parse(r#"{"a":01}"#).is_err());
    assert!(Json::parse(r#"{"a":"\ud800"}"#).is_err());
}

#[test]
fn parses_numbers() {
    let json = Json::parse("[0,-1,1.5,1e3,-2.5E-2]").unwrap();
    assert_eq!(
        json.as_array().unwrap(),
        &[
            Json::Number(0.0),
            Json::Number(-1.0),
            Json::Number(1.5),
            Json::Number(1000.0),
            Json::Number(-0.025)
        ]
    );
}

#[test]
fn rejects_deep_nesting() {
    let text = "[".repeat(200) + &"]".repeat(200);
    assert!(Json::parse(&text).is_err());
}

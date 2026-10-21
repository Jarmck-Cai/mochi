// Fixed team-owned module lifecycle regression; runtime is checked externally.
fn tokens(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() { i += 1; continue; }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' { i += 1; }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
            i = (i + 2).min(chars.len()); continue;
        }
        if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' { i += 2; }
                else if chars[i] == c { i += 1; break; }
                else { i += 1; }
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i; i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
            out.push(chars[start..i].iter().collect()); continue;
        }
        out.push(c.to_string()); i += 1;
    }
    out
}
fn contains(source: &[String], expected: &[&str]) -> bool {
    source.windows(expected.len()).any(|window| window.iter().map(String::as_str).eq(expected.iter().copied()))
}
fn body<'a>(source: &'a [String], name: &str) -> &'a [String] {
    let start = source.windows(4).position(|w| w[0] == name && w[1] == "(" && w[2] == ")" && w[3] == "{")
        .expect("module lifecycle function must have a definition") + 4;
    let mut depth = 1;
    for end in start..source.len() {
        if source[end] == "{" { depth += 1; }
        if source[end] == "}" { depth -= 1; }
        if depth == 0 { return &source[start..end]; }
    }
    panic!("unterminated module lifecycle function");
}
#[test]
fn launcher_api_is_declared_and_bound_to_module_lifetime() {
    let header = tokens(include_str!("../../ime-plugin/src/brain_client.h"));
    let module = tokens(include_str!("../../ime-plugin/src/mochi_module.cc"));
    for name in ["StartBrainLauncher", "StopBrainLauncher"] {
        assert!(contains(&header, &["void", name, "(", ")", ";"]), "missing launcher API declaration: {}", name);
    }
    assert!(contains(body(&module, "rime_mochi_initialize"), &["mochi", ":", ":", "StartBrainLauncher", "(", ")", ";"]),
        "module initialize must start the launcher");
    assert!(contains(body(&module, "rime_mochi_finalize"), &["mochi", ":", ":", "StopBrainLauncher", "(", ")", ";"]),
        "module finalize must stop the launcher");
}

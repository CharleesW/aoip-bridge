//! Atualizacao pelo GitHub: as versoes ficam nas "Releases" do repositorio do projeto
//! (https://github.com/CharleesW/aoip-bridge/releases), cada uma com os pacotes de Windows e
//! de Mac. Aqui fica a parte sem rede: comparar versoes, ler a lista de versoes que o GitHub
//! devolve (JSON) e conferir a impressao digital (SHA-256) do arquivo baixado.

/// Endereco da lista de versoes (a API publica do GitHub, sem login).
pub const RELEASES_URL: &str = "https://api.github.com/repos/CharleesW/aoip-bridge/releases?per_page=15";
/// Pagina das versoes, para o usuario ver o que mudou.
pub const RELEASES_PAGE: &str = "https://github.com/CharleesW/aoip-bridge/releases";

/// Situacao da atualizacao, para o painel e o icone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateInfo {
    /// "" (ainda nao procurou), "checking", "uptodate", "available", "downloading",
    /// "installing", "error".
    pub state: String,
    pub version: String,
    pub page: String,
    pub message: String,
}

/// "v0.6.1-teste" ou "0.6.1" -> [0, 6, 1].
pub fn parse_version(tag: &str) -> Option<Vec<u32>> {
    let core = tag.trim().trim_start_matches(['v', 'V']).split(['-', '+', ' ']).next()?;
    let parts: Option<Vec<u32>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// `candidate` e mais nova que `current`?
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

/// Uma versao disponivel para este sistema.
#[derive(Clone, Debug, PartialEq)]
pub struct Available {
    pub version: String,
    pub tag: String,
    pub page: String,
    pub asset_name: String,
    pub url: String,
    /// SHA-256 publicado pelo GitHub para o arquivo (hexadecimal).
    pub sha256: String,
}

/// Escolhe, na lista de versoes do GitHub, a mais nova que `current` que tenha pacote cujo
/// nome comeca com `asset_prefix` (ex.: "AoIP-Bridge-Windows-") e termina em ".zip".
/// Versoes sem impressao digital publicada sao ignoradas (nao da para conferir o download).
pub fn pick_release(json: &str, current: &str, asset_prefix: &str) -> Result<Option<Available>, String> {
    let root = Json::parse(json).ok_or("resposta do GitHub em formato inesperado")?;
    let list = root.as_array().ok_or("resposta do GitHub em formato inesperado")?;
    let mut best: Option<Available> = None;
    for rel in list {
        if rel.get("draft").and_then(Json::as_bool) == Some(true) {
            continue;
        }
        let Some(tag) = rel.get("tag_name").and_then(Json::as_str) else { continue };
        let Some(version) = parse_version(tag).map(|v| v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")) else { continue };
        if !is_newer(&version, current) || best.as_ref().is_some_and(|b| !is_newer(&version, &b.version)) {
            continue;
        }
        let assets = rel.get("assets").and_then(Json::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
        let found = assets.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?;
            if !(name.starts_with(asset_prefix) && name.ends_with(".zip")) {
                return None;
            }
            let url = a.get("browser_download_url")?.as_str()?;
            let sha = a.get("digest")?.as_str()?.strip_prefix("sha256:")?;
            (url.starts_with("https://github.com/") && sha.len() == 64).then(|| (name.to_string(), url.to_string(), sha.to_lowercase()))
        });
        if let Some((asset_name, url, sha256)) = found {
            let page = rel.get("html_url").and_then(Json::as_str).unwrap_or(RELEASES_PAGE).to_string();
            best = Some(Available { version, tag: tag.to_string(), page, asset_name, url, sha256 });
        }
    }
    Ok(best)
}

// ---------------------------------------------------------------- JSON minimo

/// Leitor de JSON pequeno (o nucleo nao usa bibliotecas de fora).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Option<Json> {
        let mut p = Parser { b: text.as_bytes(), i: 0 };
        let v = p.value(0)?;
        p.ws();
        (p.i == p.b.len()).then_some(v)
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_array(&self) -> Option<&Vec<Json>> {
        if let Json::Arr(a) = self { Some(a) } else { None }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
    }
    fn lit(&mut self, word: &str, v: Json) -> Option<Json> {
        self.b[self.i..].starts_with(word.as_bytes()).then(|| {
            self.i += word.len();
            v
        })
    }
    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > 64 {
            return None;
        }
        self.ws();
        match *self.b.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b'}').is_some() {
                    return Some(Json::Obj(items));
                }
                loop {
                    self.ws();
                    let Json::Str(k) = self.string()? else { return None };
                    self.eat(b':')?;
                    items.push((k, self.value(depth + 1)?));
                    if self.eat(b',').is_none() {
                        self.eat(b'}')?;
                        return Some(Json::Obj(items));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']').is_some() {
                    return Some(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.eat(b',').is_none() {
                        self.eat(b']')?;
                        return Some(Json::Arr(items));
                    }
                }
            }
            b'"' => self.string(),
            b't' => self.lit("true", Json::Bool(true)),
            b'f' => self.lit("false", Json::Bool(false)),
            b'n' => self.lit("null", Json::Null),
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(Json::Num)
            }
        }
    }
    fn string(&mut self) -> Option<Json> {
        if self.b.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(Json::Str(out)),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = |p: &mut Self| -> Option<u32> {
                                let h = std::str::from_utf8(p.b.get(p.i..p.i + 4)?).ok()?;
                                p.i += 4;
                                u32::from_str_radix(h, 16).ok()
                            };
                            let mut cp = hex(self)?;
                            if (0xD800..0xDC00).contains(&cp) && self.b.get(self.i..self.i + 2) == Some(b"\\u") {
                                self.i += 2;
                                let lo = hex(self)?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        }
                        _ => return None,
                    }
                }
                _ => {
                    // copia o caractere UTF-8 inteiro
                    let start = self.i - 1;
                    let len = match c {
                        0x00..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    self.i = (start + len).min(self.b.len());
                    out.push_str(std::str::from_utf8(&self.b[start..self.i]).ok()?);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- SHA-256

/// SHA-256 (FIPS 180-4) de `data`, em hexadecimal minusculo.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for t in 0..16 {
            w[t] = u32::from_be_bytes(block[t * 4..t * 4 + 4].try_into().unwrap());
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.6.1-teste"), Some(vec![0, 6, 1]));
        assert_eq!(parse_version("0.10.0"), Some(vec![0, 10, 0]));
        assert_eq!(parse_version("teste"), None);
        assert!(is_newer("v0.6.1-teste", "0.6.0"));
        assert!(is_newer("0.10.0", "0.9.9"), "compara numeros, nao texto");
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.6.0-teste", "0.6.0"));
        assert!(!is_newer("v0.5.9", "0.6.0"));
    }

    #[test]
    fn sha256_known_values() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(sha256_hex(&vec![b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn json_reader() {
        let j = Json::parse(r#" {"a": [1, -2.5e1, true, null], "b": "x\"yé🎵", "c": {}} "#).unwrap();
        assert_eq!(j.get("a").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(j.get("b").unwrap().as_str(), Some("x\"yé🎵"));
        assert_eq!(Json::parse("{\"a\": 1,}"), None);
        assert_eq!(Json::parse("[1 2]"), None);
        assert_eq!(Json::parse("\"Ação\"").unwrap().as_str(), Some("Ação"));
    }

    #[test]
    fn picks_the_newest_release_for_this_system() {
        let sha = "a".repeat(64);
        let json = format!(r#"[
          {{"tag_name": "v0.7.0-teste", "draft": true, "html_url": "x", "assets": []}},
          {{"tag_name": "v0.6.2-teste", "draft": false, "prerelease": true, "html_url": "https://github.com/CharleesW/aoip-bridge/releases/tag/v0.6.2-teste",
            "assets": [
              {{"name": "AoIP-Bridge-Mac-0.6.2-teste.zip", "browser_download_url": "https://github.com/CharleesW/aoip-bridge/releases/download/v0.6.2-teste/AoIP-Bridge-Mac-0.6.2-teste.zip", "digest": "sha256:{sha}"}},
              {{"name": "AoIP-Bridge-Windows-0.6.2-teste.zip", "browser_download_url": "https://github.com/CharleesW/aoip-bridge/releases/download/v0.6.2-teste/AoIP-Bridge-Windows-0.6.2-teste.zip", "digest": "sha256:{sha}"}}
            ]}},
          {{"tag_name": "v0.6.1-teste", "draft": false, "assets": [
              {{"name": "AoIP-Bridge-Windows-0.6.1-teste.zip", "browser_download_url": "https://github.com/x/y/z.zip", "digest": "sha256:{sha}"}}
            ]}},
          {{"tag_name": "v0.5.0-teste", "draft": false, "assets": []}}
        ]"#);
        let win = pick_release(&json, "0.6.0", "AoIP-Bridge-Windows-").unwrap().unwrap();
        assert_eq!((win.version.as_str(), win.asset_name.as_str()), ("0.6.2", "AoIP-Bridge-Windows-0.6.2-teste.zip"));
        assert_eq!(win.sha256, sha);
        assert!(pick_release(&json, "0.6.2", "AoIP-Bridge-Windows-").unwrap().is_none(), "ja na mais nova");
        assert!(pick_release(&json, "0.6.0", "AoIP-Bridge-Linux-").unwrap().is_none(), "sem pacote para o sistema");
        // sem impressao digital, ou fora do github.com: ignorada
        let no_digest = r#"[{"tag_name":"v9.0","assets":[{"name":"AoIP-Bridge-Mac-9.0.zip","browser_download_url":"https://github.com/a.zip"}]}]"#;
        assert!(pick_release(no_digest, "0.6.0", "AoIP-Bridge-Mac-").unwrap().is_none());
        assert!(pick_release("nao e json", "0.6.0", "x").is_err());
    }
}

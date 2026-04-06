//! Стабильная query для teach: сортировка пар (ключ, значение), как с хоста пришло — парсим здесь.

use url::form_urlencoded::parse;

/// `raw` — строка от хоста (допустим префикс `?`), пустая / отсутствует → `None`.
pub fn canonical_query(raw: Option<&str>) -> Option<String> {
    let s = raw.map(str::trim)?;
    let s = s.strip_prefix('?').unwrap_or(s).trim();
    if s.is_empty() {
        return None;
    }

    let mut pairs: Vec<(String, String)> = parse(s.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if pairs.is_empty() {
        return None;
    }

    pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in &pairs {
        ser.append_pair(k, v);
    }
    Some(ser.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permutation_same_canonical() {
        assert_eq!(
            canonical_query(Some("b=2&a=1")).as_deref(),
            canonical_query(Some("a=1&b=2")).as_deref()
        );
    }

    #[test]
    fn empty_none() {
        assert_eq!(canonical_query(None), None);
        assert_eq!(canonical_query(Some("")), None);
        assert_eq!(canonical_query(Some("?")), None);
    }
}

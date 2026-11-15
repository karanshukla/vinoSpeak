//! Kokoro's voices: what an id like `af_heart` says about a voice, and which
//! ones the downloaded model holds.

use std::path::Path;

use crate::model_cache;

/// A Kokoro voice. Its id says the rest: `af_heart` is an American English
/// (a) female (f) voice called Heart.
#[derive(Clone, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub accent: &'static str,
    pub gender: &'static str,
    /// What vinospeak's `--language` takes for it.
    pub language: &'static str,
}

/// Kokoro's language letters, in the order the picker groups them. Japanese
/// (j) and Mandarin (z) are left out: vinospeak has no language for them.
const ACCENTS: &[(char, &str, &str)] = &[
    ('a', "American English", "en-us"),
    ('b', "British English", "en-gb"),
    ('e', "Spanish", "es"),
    ('f', "French", "fr-fr"),
    ('h', "Hindi", "hi"),
    ('i', "Italian", "it"),
    ('p', "Brazilian Portuguese", "pt-br"),
];

pub fn voice(id: &str) -> Option<Voice> {
    let mut letters = id.chars();
    let (accent, gender) = (letters.next()?, letters.next()?);
    let name = id.get(3..).filter(|_| id.get(2..3) == Some("_"))?;
    let &(_, accent, language) = ACCENTS.iter().find(|(c, _, _)| *c == accent)?;
    let gender = match gender {
        'f' => "female",
        'm' => "male",
        _ => return None,
    };
    let name = name
        .split('_')
        .map(|word| {
            let mut c = word.chars();
            c.next()
                .map_or_else(String::new, |first| first.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(Voice {
        id: id.to_owned(),
        name,
        accent,
        gender,
        language,
    })
}

/// Kokoro's voices that vinospeak can speak, from the model it downloaded,
/// grouped by accent, women then men, by name.
pub fn voices() -> Vec<Voice> {
    voices_in(model_cache::dir().as_deref())
}

/// The voices in the model under `cache`, or Heart alone when there's none yet.
pub fn voices_in(cache: Option<&Path>) -> Vec<Voice> {
    let mut voices: Vec<Voice> = cache
        .and_then(|c| std::fs::read_dir(c.join(model_cache::MODEL_DIR).join("voices")).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let file = e.ok()?.file_name().into_string().ok()?;
            voice(file.strip_suffix(".bin")?)
        })
        .collect();
    let rank = |v: &Voice| ACCENTS.iter().position(|a| a.1 == v.accent);
    voices.sort_by(|a, b| (rank(a), a.gender, &a.name).cmp(&(rank(b), b.gender, &b.name)));
    if voices.is_empty() {
        voices.extend(voice("af_heart"));
    }
    voices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_ids_become_names() {
        let v = voice("af_heart").unwrap();
        assert_eq!(
            (v.name.as_str(), v.accent, v.gender, v.language),
            ("Heart", "American English", "female", "en-us")
        );
        let v = voice("bm_george").unwrap();
        assert_eq!(
            (v.accent, v.gender, v.language),
            ("British English", "male", "en-gb")
        );
        assert_eq!(voice("pm_alex").unwrap().language, "pt-br");
        assert_eq!(voice("af_two_words").unwrap().name, "Two Words");
        assert!(voice("jf_alpha").is_none(), "no Japanese in vinospeak");
        assert!(voice("zm_yunxi").is_none(), "no Mandarin in vinospeak");
        assert!(voice("ax_odd").is_none());
        assert!(voice("af").is_none());
    }
}

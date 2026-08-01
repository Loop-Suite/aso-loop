//! 결정론적 검사. LLM 미사용, 룰 기반.
//! (근거: 평가 비용 위계 — assertion/코드 규칙 → LLM judge 순으로 싸고 안정적.
//!  bizplan-loop의 checks.rs와 동일한 설계를 ASO 도메인에 맞게 재작성했다.)
//!
//! ## 오픈소스 포팅 출처
//! `normalize_keyword` / `sanitize_keywords` / `normalize_text_for_match`는
//! [semihcihan/App-Store-Optimization-CLI](https://github.com/semihcihan/App-Store-Optimization-CLI)
//! (MIT License)의 다음 로직을 참고해 Rust로 재작성했다(코드 복사가 아니라 알고리즘만 포팅):
//! - `cli/domain/keywords/policy.ts` → `normalizeKeyword`(trim+lowercase), `sanitizeKeywords`(정규화 후 Set로 dedup)
//! - `cli/shared/aso-keyword-utils.ts` → `normalizeTextForKeywordMatch`(유니코드 정규화 후 문자/숫자 이외를 공백으로 치환, 공백 정리)
//!
//! 원본은 `.normalize("NFKC")` + `\p{L}\p{N}\p{M}` 유니코드 정규식을 쓰지만, 이 프로젝트는
//! `unicode-normalization` 크레이트를 추가하지 않고 `char::is_alphanumeric()` 기반으로
//! 근사했다 — 완전한 NFKC 동등은 아니다.

use crate::spec::{Section, Spec, Store};
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Metrics {
    pub total_chars: usize,
    pub field_chars: BTreeMap<String, usize>,
    pub keyword_coverage: usize,
    pub keyword_total: usize,
    pub matched_keywords: Vec<String>,
    /// Apple dedup 대상 필드(title/subtitle/keywords 등) 사이에 겹치는 토큰
    pub duplicate_keywords: Vec<String>,
    pub emoji_count: usize,
    pub banned_hits: Vec<String>,
}

fn norm_head(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `#`로 시작하는 헤딩 기준으로 (헤딩, 본문) 분할. (bizplan-loop 구조 재사용)
pub fn split_sections(doc: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut cur_head = String::new();
    let mut cur_body = String::new();
    for line in doc.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            if !cur_head.is_empty() || !cur_body.trim().is_empty() {
                out.push((cur_head.clone(), cur_body.clone()));
            }
            cur_head = t.trim_start_matches('#').trim().to_string();
            cur_body.clear();
        } else {
            cur_body.push_str(line);
            cur_body.push('\n');
        }
    }
    if !cur_head.is_empty() || !cur_body.trim().is_empty() {
        out.push((cur_head, cur_body));
    }
    out
}

/// 문서에서 스펙 필드 id -> 본문 텍스트(트림) 매핑.
pub fn field_bodies(spec: &Spec, doc: &str) -> BTreeMap<String, String> {
    let secs = split_sections(doc);
    let mut map = BTreeMap::new();
    for s in &spec.sections {
        let want = norm_head(&s.title);
        if let Some((_, body)) = secs.iter().find(|(h, _)| {
            !h.is_empty() && (norm_head(h).contains(&want) || (want.contains(&norm_head(h)) && !h.is_empty()))
        }) {
            map.insert(s.id.clone(), body.trim().to_string());
        }
    }
    map
}

// ---- MIT(semihcihan/App-Store-Optimization-CLI) 포팅 ----

/// 키워드 정규화: trim + lowercase.
/// 포팅 출처: cli/domain/keywords/policy.ts::normalizeKeyword
pub fn normalize_keyword(k: &str) -> String {
    k.trim().to_lowercase()
}

/// 정규화 후 중복 제거(순서 유지).
/// 포팅 출처: cli/domain/keywords/policy.ts::sanitizeKeywords
pub fn sanitize_keywords(input: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for k in input {
        let n = normalize_keyword(k);
        if !n.is_empty() && seen.insert(n.clone()) {
            out.push(n);
        }
    }
    out
}

/// 키워드 매칭용 텍스트 정규화: 소문자화 + 영숫자/공백 이외 문자를 공백으로 치환 + 공백 정리.
/// 포팅 출처: cli/shared/aso-keyword-utils.ts::normalizeTextForKeywordMatch
pub fn normalize_text_for_match(text: &str) -> String {
    let replaced: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    replaced.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- ASO 도메인 검사 ----

pub fn metrics(spec: &Spec, doc: &str) -> Metrics {
    let bodies = field_bodies(spec, doc);
    let mut field_chars = BTreeMap::new();
    let mut total_chars = 0usize;
    for s in &spec.sections {
        let n = bodies.get(&s.id).map(|b| b.chars().count()).unwrap_or(0);
        field_chars.insert(s.id.clone(), n);
        total_chars += n;
    }

    let full_text = bodies.values().cloned().collect::<Vec<_>>().join(" ");
    let normalized_doc = normalize_text_for_match(&full_text);

    let mut matched_keywords = Vec::new();
    for kw in &spec.target_keywords {
        let nk_match = normalize_text_for_match(&normalize_keyword(kw));
        if !nk_match.is_empty() && normalized_doc.contains(&nk_match) {
            matched_keywords.push(kw.clone());
        }
    }

    Metrics {
        total_chars,
        field_chars,
        keyword_coverage: matched_keywords.len(),
        keyword_total: spec.target_keywords.len(),
        matched_keywords,
        duplicate_keywords: duplicate_keywords_across_fields(spec, &bodies),
        emoji_count: full_text.chars().filter(|c| is_emoji(*c)).count(),
        banned_hits: banned_hits(spec, &full_text),
    }
}

/// dedup 대상 필드(title/subtitle/keywords 등) 사이에 겹치는 단어(토큰) 목록.
/// Apple은 색인 시 title+subtitle+keywords를 자동으로 dedup하므로,
/// 같은 키워드를 여러 필드에 중복 배치하면 글자수만 낭비된다.
fn duplicate_keywords_across_fields(spec: &Spec, bodies: &BTreeMap<String, String>) -> Vec<String> {
    let targets: Vec<&Section> = spec.sections.iter().filter(|s| s.keyword_dedup_target).collect();
    if targets.len() < 2 {
        return Vec::new();
    }
    let mut token_field_count: BTreeMap<String, usize> = BTreeMap::new();
    for s in &targets {
        let body = bodies.get(&s.id).cloned().unwrap_or_default();
        let normalized = normalize_text_for_match(&body);
        let mut seen_in_field = HashSet::new();
        for tok in normalized.split(' ') {
            if tok.len() < 2 {
                continue; // 조사·단일문자 노이즈 제외
            }
            if seen_in_field.insert(tok.to_string()) {
                *token_field_count.entry(tok.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut dups: Vec<String> = token_field_count.into_iter().filter(|(_, n)| *n >= 2).map(|(t, _)| t).collect();
    dups.sort();
    dups
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0x2190..=0x21FF | 0x2B00..=0x2BFF | 0xFE0F | 0x1F1E6..=0x1F1FF
    )
}

fn default_superlative_patterns() -> &'static [&'static str] {
    &[
        r"\bbest\b",
        r"\btop\s*1\b",
        r"#\s*1\b",
        r"\bno\.?\s*1\b",
        r"1\s*위",
        r"최고",
        r"최초",
        r"유일",
        r"업계\s*1위",
        r"가장\s*(좋은|빠른|정확한)",
    ]
}

fn default_price_patterns() -> &'static [&'static str] {
    &[r"\$\s*\d", r"\d+\s*%\s*(off|할인)", r"무료\s*체험", r"무료\s*다운로드", r"세일", r"특가", r"이벤트가"]
}

fn compile_all(patterns: &[&str]) -> Vec<Regex> {
    patterns.iter().filter_map(|p| RegexBuilder::new(p).case_insensitive(true).build().ok()).collect()
}

/// 스펙의 competitor/trademark 패턴 + 기본 최상급/가격 패턴에 매치되는 원문 조각을 수집.
fn banned_hits(spec: &Spec, text: &str) -> Vec<String> {
    static SUPER_RE: OnceLock<Vec<Regex>> = OnceLock::new();
    static PRICE_RE: OnceLock<Vec<Regex>> = OnceLock::new();
    let super_res = SUPER_RE.get_or_init(|| compile_all(default_superlative_patterns()));
    let price_res = PRICE_RE.get_or_init(|| compile_all(default_price_patterns()));
    let user_terms: Vec<&str> = spec.banned_terms.iter().map(|s| s.as_str()).collect();
    let user_res = compile_all(&user_terms);

    let mut hits = Vec::new();
    for (label, res) in [
        ("최상급표현", super_res.as_slice()),
        ("가격문구", price_res.as_slice()),
        ("금지어(스펙 지정)", user_res.as_slice()),
    ] {
        for re in res {
            if let Some(m) = re.find(text) {
                hits.push(format!("{}: \"{}\"", label, m.as_str()));
            }
        }
    }
    hits
}

pub fn missing_required(spec: &Spec, doc: &str) -> Vec<String> {
    let bodies = field_bodies(spec, doc);
    spec.sections
        .iter()
        .filter(|s| s.required && bodies.get(&s.id).map(|b| b.is_empty()).unwrap_or(true))
        .map(|s| s.title.clone())
        .collect()
}

/// 형식·분량·키워드·금지어 관련 결정론적 지적 사항.
pub fn format_issues(spec: &Spec, doc: &str) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();
    let bodies = field_bodies(spec, doc);

    for m in missing_required(spec, doc) {
        issues.push(format!("필수 필드 '{}' 누락 → 작성 필요", m));
    }

    for s in &spec.sections {
        let n = bodies.get(&s.id).map(|b| b.chars().count()).unwrap_or(0);
        if n == 0 {
            continue; // 누락은 위에서 이미 처리
        }
        if n > s.max_chars {
            issues.push(format!(
                "'{}' 글자수 초과: {}자 (최대 {}자) → 스토어 등록 시 잘리거나 반려될 수 있음, 압축 필요",
                s.title, n, s.max_chars
            ));
        } else if s.min_chars > 0 && n < s.min_chars {
            issues.push(format!(
                "'{}' 글자수 부족: {}자 (권장 {}자 이상) → 노출 기회 낭비 가능",
                s.title, n, s.min_chars
            ));
        }
        // Apple 키워드 필드는 글자수 기준이 실제로는 byte 기준일 수 있다는 보고가 있어 불확실.
        if spec.store == Store::Apple && s.id == "keywords" && n as f64 > s.max_chars as f64 * 0.9 {
            issues.push(format!(
                "[불확실] keywords 필드가 {}자로 상한({}자)에 근접 — Apple 키워드 필드가 자수/바이트 중 \
                 무엇을 기준으로 하는지 문서상 불명확하므로 App Store Connect에서 직접 확인 권장 \
                 (한글 등 멀티바이트 문자 사용 시 특히 주의)",
                n, s.max_chars
            ));
        }
    }

    if !spec.target_keywords.is_empty() {
        let deduped = sanitize_keywords(&spec.target_keywords);
        if deduped.len() < spec.target_keywords.len() {
            issues.push(format!(
                "스펙의 target_keywords에 중복 항목이 있음({}개 → 정규화 후 {}개) → TOML에서 정리 권장",
                spec.target_keywords.len(),
                deduped.len()
            ));
        }
    }

    let m = metrics(spec, doc);
    if !m.duplicate_keywords.is_empty() {
        issues.push(format!(
            "필드 간 키워드 중복 {}건: {} → 자동 dedup 대상 필드에 중복 배치는 글자수 낭비",
            m.duplicate_keywords.len(),
            m.duplicate_keywords.join(", ")
        ));
    }
    if !spec.target_keywords.is_empty() && m.keyword_coverage < m.keyword_total {
        let missing: Vec<&str> = spec.target_keywords.iter().filter(|k| !m.matched_keywords.contains(k)).map(|s| s.as_str()).collect();
        issues.push(format!(
            "타겟 키워드 커버리지 {}/{} → 미반영: {}",
            m.keyword_coverage,
            m.keyword_total,
            missing.join(", ")
        ));
    }
    if m.emoji_count > spec.emoji_max {
        issues.push(format!("이모지 {}개 사용 (허용 {}개) → 과도한 이모지는 스팸으로 인식될 수 있음, 축소", m.emoji_count, spec.emoji_max));
    }
    for h in &m.banned_hits {
        issues.push(format!("금지 표현 감지 — {} → 표현 교체 필요(상표권·과장광고 리스크)", h));
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_text_for_match_strips_punct_and_lowers() {
        assert_eq!(normalize_text_for_match("Hello, World!!"), "hello world");
        assert_eq!(normalize_text_for_match("가계부  #1  앱"), "가계부 1 앱");
    }

    #[test]
    fn sanitize_keywords_dedups_case_insensitively() {
        let v = vec!["Budget".to_string(), " budget ".to_string(), "Tracker".to_string()];
        assert_eq!(sanitize_keywords(&v), vec!["budget".to_string(), "tracker".to_string()]);
    }

    fn test_spec() -> Spec {
        use crate::spec::Criterion;
        Spec {
            name: "테스트".into(),
            store: Store::Apple,
            context: String::new(),
            scoring_source: String::new(),
            target_keywords: vec!["가계부".into(), "지출관리".into()],
            banned_terms: vec!["뱅크샐러드".into()],
            emoji_max: 1,
            angles: vec![],
            bands: vec![],
            sections: vec![
                Section { id: "title".into(), title: "Title".into(), guide: String::new(), max_chars: 10, min_chars: 0, required: true, keyword_dedup_target: true },
                Section { id: "subtitle".into(), title: "Subtitle".into(), guide: String::new(), max_chars: 10, min_chars: 0, required: true, keyword_dedup_target: true },
            ],
            criteria: vec![Criterion { id: "x".into(), name: "x".into(), weight: 1.0, guide: String::new() }],
        }
    }

    #[test]
    fn format_issues_flags_overlength_and_missing_and_coverage() {
        let spec = test_spec();
        // title 10자 초과, subtitle 누락, 키워드 1개만 반영, 뱅크샐러드 금지어 포함
        let doc = "## Title\n가계부 지출관리 완전정복판\n";
        let issues = format_issues(&spec, doc);
        assert!(issues.iter().any(|i| i.contains("Title") && i.contains("초과")));
        assert!(issues.iter().any(|i| i.contains("Subtitle") && i.contains("누락")));
    }

    #[test]
    fn format_issues_flags_banned_term_and_duplicate_keyword() {
        let spec = test_spec();
        let doc = "## Title\n가계부\n## Subtitle\n가계부 앱\n";
        let issues = format_issues(&spec, doc);
        assert!(issues.iter().any(|i| i.contains("중복")));
    }

    #[test]
    fn metrics_counts_keyword_coverage() {
        let spec = test_spec();
        let doc = "## Title\n가계부\n## Subtitle\n지출관리 완벽\n";
        let m = metrics(&spec, doc);
        assert_eq!(m.keyword_coverage, 2);
        assert_eq!(m.keyword_total, 2);
    }
}

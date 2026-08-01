use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 스토어. 필드 구성만 다르고 검사·채점 로직은 공유한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Apple,
    Google,
}

impl Store {
    pub fn label(&self) -> &'static str {
        match self {
            Store::Apple => "Apple App Store",
            Store::Google => "Google Play",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Spec {
    /// 앱/캠페인 이름
    pub name: String,
    pub store: Store,
    /// 앱 개요·톤앤매너 등 맥락. 프롬프트에 그대로 삽입됨.
    #[serde(default)]
    pub context: String,
    /// 가중치 근거 메모(리포트에 표시).
    #[serde(default)]
    pub scoring_source: String,
    /// 타겟 키워드 리스트. 커버리지 검사에 사용.
    #[serde(default)]
    pub target_keywords: Vec<String>,
    /// 경쟁 앱명·상표명 등 사용자 정의 금지어 패턴(정규식, 대소문자 무시).
    #[serde(default)]
    pub banned_terms: Vec<String>,
    /// 허용 이모지 최대 개수. 초과 시 지적.
    #[serde(default = "default_emoji_max")]
    pub emoji_max: usize,
    /// 생성 다양성을 위한 접근 각도.
    #[serde(default)]
    pub angles: Vec<String>,
    /// 점수대 서술자(0~100). 미지정 시 기본값 사용.
    #[serde(default)]
    pub bands: Vec<String>,
    pub sections: Vec<Section>,
    pub criteria: Vec<Criterion>,
}

/// ASO 리스팅 필드 1개(title/subtitle/keywords/promo_text/description 등).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Section {
    pub id: String,
    /// 문서의 `## 제목` 헤딩과 매칭되는 표시명.
    pub title: String,
    #[serde(default)]
    pub guide: String,
    /// 스토어가 강제하는 최대 글자 수(하드 리밋).
    pub max_chars: usize,
    /// 권장 최소 글자 수. 0이면 검사 안 함(활용도 낮음 경고용, 강제 아님).
    #[serde(default)]
    pub min_chars: usize,
    #[serde(default = "default_true")]
    pub required: bool,
    /// true면 title/subtitle/keywords처럼 Apple이 필드 간 자동 dedup하는
    /// 대상으로 취급해 다른 dedup 대상 필드와 키워드 중복을 검사한다.
    #[serde(default)]
    pub keyword_dedup_target: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Criterion {
    pub id: String,
    pub name: String,
    /// 가중치. 합이 1이 아니어도 내부에서 정규화.
    pub weight: f64,
    #[serde(default)]
    pub guide: String,
}

fn default_true() -> bool {
    true
}

fn default_emoji_max() -> usize {
    3
}

pub const DEFAULT_BANDS: &[&str] = &[
    "90~100: 검색 상위 노출과 전환을 동시에 잡는 카피. 타겟 키워드가 자연스럽게 녹아 있고 CTA가 명확하며 규정 위반이 없음.",
    "75~89: 실전 투입 가능 수준. 핵심 키워드는 반영됐으나 일부 문구가 밋밋하거나 현지화가 매끄럽지 않음.",
    "60~74: 초안 수준. 구조는 갖췄으나 키워드 배치가 산발적이고 전환 문구가 일반론에 머무름.",
    "40~59: 재작업 필요. 키워드 반영이 얕고 스팸성 나열이나 상투적 문구 위주.",
    "0~39: 사용 불가. 글자수 규정 위반이 많거나 심사기준과 무관한 내용.",
];

impl Spec {
    pub fn load(path: &Path) -> Result<Spec> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("스펙 파일 읽기 실패: {}", path.display()))?;
        let spec: Spec = toml::from_str(&s)
            .with_context(|| format!("스펙 TOML 파싱 실패: {}", path.display()))?;
        anyhow::ensure!(!spec.sections.is_empty(), "sections 비어 있음");
        anyhow::ensure!(!spec.criteria.is_empty(), "criteria 비어 있음");
        anyhow::ensure!(
            spec.criteria.iter().all(|c| c.weight > 0.0),
            "criteria weight는 모두 0보다 커야 함"
        );
        anyhow::ensure!(
            spec.sections.iter().all(|s| s.max_chars > 0),
            "sections의 max_chars는 모두 0보다 커야 함"
        );
        let mut ids: Vec<&str> = spec.criteria.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        anyhow::ensure!(ids.len() == n, "criteria id 중복");
        Ok(spec)
    }

    pub fn weight_sum(&self) -> f64 {
        self.criteria.iter().map(|c| c.weight).sum()
    }

    pub fn bands_prompt(&self) -> String {
        if self.bands.is_empty() {
            DEFAULT_BANDS.join("\n")
        } else {
            self.bands.join("\n")
        }
    }

    pub fn sections_prompt(&self) -> String {
        self.sections
            .iter()
            .map(|s| {
                let mut line = format!("## {}\n- 작성지침: {}\n- 최대 {}자", s.title, s.guide, s.max_chars);
                if s.min_chars > 0 {
                    line.push_str(&format!(" (권장 {}자 이상)", s.min_chars));
                }
                if s.required {
                    line.push_str("\n- 필수 필드");
                } else {
                    line.push_str("\n- 선택 필드(비워도 됨)");
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn rubric_prompt(&self) -> String {
        let sum = self.weight_sum();
        self.criteria
            .iter()
            .map(|c| {
                format!(
                    "- id=\"{}\" | {} (배점 비중 {:.0}%) : {}",
                    c.id,
                    c.name,
                    c.weight / sum * 100.0,
                    c.guide
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn keywords_prompt(&self) -> String {
        if self.target_keywords.is_empty() {
            "(지정된 타겟 키워드 없음)".to_string()
        } else {
            self.target_keywords.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn spec_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("specs").join(name)
    }

    #[test]
    fn example_apple_spec_loads_and_normalizes() {
        let sp = Spec::load(&spec_path("example-apple.toml")).expect("apple 스펙 로드 실패");
        assert_eq!(sp.store, Store::Apple);
        assert!(sp.sections.iter().any(|s| s.id == "keywords" && s.max_chars == 100));
        assert!((sp.weight_sum() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn example_google_spec_loads_and_normalizes() {
        let sp = Spec::load(&spec_path("example-google.toml")).expect("google 스펙 로드 실패");
        assert_eq!(sp.store, Store::Google);
        assert!(sp.sections.iter().any(|s| s.id == "short_description" && s.max_chars == 80));
        assert!((sp.weight_sum() - 100.0).abs() < 1e-9);
    }
}

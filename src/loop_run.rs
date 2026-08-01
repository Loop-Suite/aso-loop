use crate::generate;
use crate::llm::Llm;
use crate::report;
use crate::score::{self, Scored};
use crate::spec::Spec;
use anyhow::Result;
use std::path::Path;

pub struct LoopOutcome {
    pub best_label: String,
    pub best_doc: String,
    pub best_score: Scored,
    pub first_doc: String,
    pub history: Vec<Scored>,
    pub stop_reason: String,
    /// 길이 인플레이션 경고(점수 대비 분량 증가)
    pub warnings: Vec<String>,
}

pub struct LoopCfg {
    pub target: f64,
    pub max_iter: usize,
    pub rounds: usize,
    /// 직전 최고점 대비 이 값 미만으로 개선되면 정체로 본다.
    pub min_delta: f64,
    /// 정체가 이 횟수 연속이면 조기 종료.
    pub patience: usize,
}

/// 생성 → 채점 → 피드백 반영 재생성 루프.
/// 반환은 마지막 회차가 아니라 전 회차 중 최고점(argmax)이다.
pub fn run(gen_llm: &Llm, judges: &[Llm], spec: &Spec, idea: &str, out_dir: &Path, cfg: &LoopCfg, angle: &str) -> Result<LoopOutcome> {
    let mut doc = generate::generate(gen_llm, spec, idea, angle)?;
    let mut history: Vec<Scored> = Vec::new();
    let mut docs: Vec<String> = Vec::new();
    let mut best_i = 0usize;
    let mut stall = 0usize;
    let mut stop_reason = format!("최대 반복 {}회 도달", cfg.max_iter.max(1));

    for i in 0..cfg.max_iter.max(1) {
        let label = format!("iter{:02}", i + 1);
        std::fs::write(out_dir.join(format!("{}.md", label)), &doc)?;

        let s = score::score_doc(judges, spec, &label, &doc, cfg.rounds, Some(idea))?;
        report::append_jsonl(out_dir, &s)?;
        println!(
            "  [{}] {:.1}/100  ({}자{})",
            label,
            s.total,
            s.metrics.total_chars,
            if s.format_issues.is_empty() { String::new() } else { format!(", 형식지적 {}건", s.format_issues.len()) }
        );

        let prev_best = history.get(best_i).map(|b: &Scored| b.total);
        let improved = match prev_best {
            None => true,
            Some(b) => s.total > b,
        };
        history.push(s.clone());
        docs.push(doc.clone());
        if improved {
            let gain = s.total - prev_best.unwrap_or(f64::NEG_INFINITY);
            best_i = history.len() - 1;
            if prev_best.is_some() && gain < cfg.min_delta {
                stall += 1;
            } else {
                stall = 0;
            }
        } else {
            stall += 1;
        }

        if s.total >= cfg.target && s.format_issues.is_empty() {
            stop_reason = format!("목표 {:.0}점 도달", cfg.target);
            break;
        }
        if stall >= cfg.patience {
            stop_reason = format!("개선 정체({}회 연속 +{:.1}점 미만)", cfg.patience, cfg.min_delta);
            break;
        }
        if i + 1 == cfg.max_iter.max(1) {
            break;
        }

        let fb = score::feedback_text(&history[history.len() - 1]);
        let weak = score::weak_points(spec, &history[history.len() - 1]);
        doc = generate::revise(gen_llm, spec, idea, &doc, &fb, &weak)?;
    }

    let best_score = history[best_i].clone();
    let best_doc = docs[best_i].clone();
    std::fs::write(out_dir.join("best.md"), &best_doc)?;

    // 길이 인플레이션 canary: 점수 대비 총 글자수가 과도하게 늘면 verbosity gaming 의심.
    // (ASO 필드는 하드 캡이 있어 bizplan-loop만큼 폭주하진 않지만, 여러 필드를 최대치까지
    //  채우는 방식으로 같은 편법이 나타날 수 있어 동일 canary를 유지한다)
    let mut warnings = Vec::new();
    let first = &history[0];
    let d_score = best_score.total - first.total;
    let d_chars = best_score.metrics.total_chars as f64 - first.metrics.total_chars as f64;
    let growth = if first.metrics.total_chars > 0 { d_chars / first.metrics.total_chars as f64 } else { 0.0 };
    if growth > 0.25 && d_score < 5.0 {
        warnings.push(format!(
            "길이 canary: 총 글자수 +{:.0}% 인데 점수는 +{:.1}점 → 내용 보강이 아니라 늘려쓰기일 가능성",
            growth * 100.0,
            d_score
        ));
    }
    if best_i + 1 < history.len() {
        warnings.push(format!(
            "마지막 회차({:.1}점)가 최고점이 아님 → best.md는 iter{:02}",
            history.last().map(|h| h.total).unwrap_or(0.0),
            best_i + 1
        ));
    }

    // 회차 간 드리프트 지표(Correlated Proxies, arXiv:2403.03185 응용): 점수는 올랐는데
    // 문서가 이전 회차와 거의 무관하게 바뀌었다면, judge가 "그럴듯함" 패턴에 맞춰 문서를
    // 갈아엎었을 뿐 실제 개선과는 무관할 위험이 있다 — 길이 canary(분량)와는 독립적으로,
    // 내용 자체의 변화량을 본다. 길이 canary와 마찬가지로 이건 결정론적 신호일 뿐이고
    // held-out gate처럼 "무엇이 옳은지"를 판정하지는 않는다.
    for i in 1..docs.len() {
        let d_score_round = history[i].total - history[i - 1].total;
        if d_score_round <= 0.0 {
            continue; // 점수가 오르지 않았다면 "점수만 오른" 케이스가 아니므로 대상 아님
        }
        let sim = jaccard_similarity(&docs[i - 1], &docs[i]);
        // 임계값 0.3: ASO 필드는 스토어 글자수 상한으로 길이가 짧게 제약돼 있어, 문장을
        // 일부 다듬는 정상적인 재작성도 공통 토큰(조사·핵심 키워드 등)을 상당수 유지하는
        // 경향이 있다. 자카드 유사도가 0.3 미만이면 "표현을 다듬음" 수준을 넘어 사실상
        // 별개 문서로 교체된 것으로 보고 경고한다(엄밀한 통계적 근거는 없음 — 보수적으로 잡은 값).
        if sim < 0.3 {
            warnings.push(format!(
                "드리프트 경고: iter{:02}→iter{:02} 점수는 {:+.1}점 올랐지만 토큰 자카드 유사도 {:.2} \
                 (0.3 미만) → 내용이 급격히 바뀌었는데 점수만 오른 것일 수 있음, 실제 카피 확인 권장",
                i, i + 1, d_score_round, sim
            ));
        }
    }

    Ok(LoopOutcome { best_label: best_score.label.clone(), best_doc, first_doc: docs[0].clone(), best_score, history, stop_reason, warnings })
}

/// 공백 기준 토큰 집합의 자카드 유사도(교집합/합집합 크기 비율). 외부 크레이트 추가 없이
/// 직접 구현 — 형태소 분석이 아니라 단순 토큰 집합 비교라 근사치다.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let ta: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let tb: std::collections::HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let inter = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::jaccard_similarity;

    #[test]
    fn jaccard_similarity_identical_is_one() {
        assert_eq!(jaccard_similarity("가계부 지출관리 앱", "가계부 지출관리 앱"), 1.0);
    }

    #[test]
    fn jaccard_similarity_disjoint_is_zero() {
        assert_eq!(jaccard_similarity("가계부 지출관리", "완전히 다른 문서"), 0.0);
    }

    #[test]
    fn jaccard_similarity_partial_overlap() {
        // {가계부, 지출관리, 앱} vs {가계부, 지출관리, 완전판} → 교집합 2, 합집합 4
        let sim = jaccard_similarity("가계부 지출관리 앱", "가계부 지출관리 완전판");
        assert!((sim - 0.5).abs() < 1e-9, "{sim}");
    }
}

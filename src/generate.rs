use crate::llm::Llm;
use crate::spec::Spec;
use anyhow::Result;

pub const SYSTEM: &str = "당신은 App Store Connect와 Google Play Console의 리스팅 규정에 정통한 \
ASO(App Store Optimization) 카피라이터다. 스토어가 강제하는 글자수 상한을 절대 넘기지 않고, \
타겟 키워드를 자연스럽게 녹여 넣으면서도 키워드 나열(stuffing)처럼 읽히지 않게 쓴다. \
상표권 있는 경쟁 앱명, '최고/1위/유일' 같은 근거 없는 최상급 표현, 가격·할인 문구는 쓰지 않는다.";

/// 최초 생성 프롬프트.
pub fn build_prompt(spec: &Spec, idea: &str, angle: &str) -> String {
    let mut p = String::new();
    p.push_str("# 과제\n아래 스토어 규격에 맞춰 앱스토어 리스팅 카피 초안을 작성하라.\n\n");
    p.push_str(&format!("## 대상 스토어: {}\n## 앱: {}\n{}\n\n", spec.store.label(), spec.name, spec.context));
    if !angle.is_empty() {
        p.push_str(&format!("## 이번 초안의 차별화 각도\n{}\n\n", angle));
    }
    p.push_str(&format!("## 앱 개요 자료\n{}\n\n", idea));
    p.push_str(&format!("## 타겟 키워드(가능한 자연스럽게 반영)\n{}\n\n", spec.keywords_prompt()));
    p.push_str(&format!("## 작성해야 할 필드\n{}\n\n", spec.sections_prompt()));
    p.push_str(&format!("## 심사 기준(작성 시 반드시 의식할 것)\n{}\n\n", spec.rubric_prompt()));
    p.push_str(
        "## 출력 규칙\n\
         - 마크다운으로 출력. 각 필드는 위 이름 그대로 `## 필드명` 헤딩 사용, 그 아래 본문만 작성.\n\
         - 서론·설명·메타코멘트 없이 문서 본문만 출력.\n\
         - 각 필드의 최대 글자수를 절대 넘기지 말 것(줄바꿈·공백 포함해서 세어 스스로 검산할 것).\n\
         - 경쟁 앱명, 상표명, '최고/1위/유일/no.1' 류 최상급 표현, 가격·할인 문구, 과도한 이모지를 쓰지 말 것.\n\
         - keywords 필드가 있다면 title/subtitle에 이미 쓴 단어를 반복하지 말 것(자동 dedup되어 낭비).\n",
    );
    p
}

/// 채점 피드백 반영 재생성 프롬프트.
pub fn build_revise_prompt(spec: &Spec, idea: &str, prev_doc: &str, feedback: &str, weak: &str) -> String {
    let mut p = String::new();
    p.push_str("# 과제\n아래 리스팅 카피 초안을 심사 피드백에 따라 개선하여 전체를 다시 출력하라.\n\n");
    p.push_str(&format!("## 대상 스토어: {}\n## 앱: {}\n{}\n\n", spec.store.label(), spec.name, spec.context));
    p.push_str(&format!("## 앱 개요 자료\n{}\n\n", idea));
    p.push_str(&format!("## 현재 초안\n{}\n\n", prev_doc));
    p.push_str(&format!("## 심사 피드백(반드시 반영)\n{}\n\n", feedback));
    if !weak.is_empty() {
        p.push_str(&format!("## 특히 점수가 낮은 항목\n{}\n\n", weak));
    }
    p.push_str(&format!("## 타겟 키워드\n{}\n\n", spec.keywords_prompt()));
    p.push_str(&format!("## 심사 기준\n{}\n\n", spec.rubric_prompt()));
    p.push_str(&format!("## 유지해야 할 필드 구조와 글자수 상한\n{}\n\n", spec.sections_prompt()));
    p.push_str(
        "## 출력 규칙\n\
         - 개선된 문서 전체를 마크다운으로 출력. 변경 요약이나 메타코멘트 금지.\n\
         - 잘 작성된 부분은 유지하고, 지적된 부분만 실질적으로 보강.\n\
         - 각 필드 글자수 상한을 반드시 지킬 것. 상한 내에서 의미 없이 늘려쓰기로 대응하지 말고 \
           약한 문장을 실질적으로 교체할 것.\n\
         - 키워드를 억지로 밀어넣어 문장이 부자연스러워지지 않게 할 것(가독성 vs 키워드 밀도 균형).\n",
    );
    p
}

pub fn generate(llm: &Llm, spec: &Spec, idea: &str, angle: &str) -> Result<String> {
    let prompt = build_prompt(spec, idea, angle);
    llm.text(&prompt, Some(SYSTEM))
}

pub fn revise(llm: &Llm, spec: &Spec, idea: &str, prev_doc: &str, feedback: &str, weak: &str) -> Result<String> {
    let prompt = build_revise_prompt(spec, idea, prev_doc, feedback, weak);
    llm.text(&prompt, Some(SYSTEM))
}

/// angles가 부족하면 기본 각도로 채워 n개 반환.
pub fn angles_for(spec: &Spec, n: usize) -> Vec<String> {
    let defaults = [
        "핵심 기능·카테고리 키워드를 title 맨 앞부분에 배치해 검색 노출을 최우선으로 한다.",
        "감성적 베네핏과 사용 후 변화를 전면에 세워 전환율을 최우선으로 한다.",
        "타겟 키워드 커버리지를 최대한 촘촘히 채우되 자연스러운 문장을 유지한다.",
        "경쟁 앱 대비 차별화 포인트(기능·가격정책·UX)를 전면에 세운다.",
        "번역투를 피하고 현지 사용자에게 자연스러운 표현을 최우선으로 한다.",
        "간결성과 가독성을 우선하고 키워드 밀도는 낮게 유지한다.",
    ];
    let pool: Vec<String> = if spec.angles.is_empty() { defaults.iter().map(|s| s.to_string()).collect() } else { spec.angles.clone() };
    (0..n).map(|i| pool[i % pool.len()].clone()).collect()
}

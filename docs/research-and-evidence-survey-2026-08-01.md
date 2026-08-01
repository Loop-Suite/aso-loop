# aso-loop 리서치·근거 서베이 (2026-08-01)

## 1. 개요

`aso-loop`은 `Loop-Suite/bizplan-loop`의 "생성 N개 → 결정론적 룰체크 → LLM 루브릭 채점(de-anchoring) → 피드백 기반 재생성" 패턴을 ASO(App Store Optimization) 리스팅 카피 도메인으로 이식한 Rust CLI다. 파이프라인 실체는 `src/checks.rs`(결정론적 검사: 글자수·키워드 커버리지·필드 간 중복·금지어), `src/score.rs`(LLM 루브릭 채점: winning_conditions 선(先)기술 → 원문 인용 강제 → 다중 라운드·다중 모델 절사평균), `src/loop_run.rs`(피드백 재생성 루프 + 길이 인플레이션 canary), `src/main.rs`의 `--gate-model`(루프 미참여 모델의 held-out 재채점)로 구성된다.

이 문서의 목적은 두 가지다.

1. 최초 리서치 라운드에서 "확인했다"고 적어둔 사실(semihcihan/App-Store-Optimization-CLI에 유사 아키텍처가 없다는 결론, furkancingoz/aso-skill 미검토, fastlane deliver의 하드코딩 상수 부재, Apple 공식 OpenAPI 스펙에 maxLength 없음)을 **이번엔 실제 소스 파일과 1차 자료를 직접 열어 재검증**하고, 틀린 부분이 있으면 자기교정한다.
2. 아직 조사하지 않은 영역 — 상용 AI 카피 에이전트, OSS 에이전트 프레임워크(LangGraph/CrewAI)의 ASO 적용 사례, reward-hacking 방지 학술 근거, Apple 글자수 제한의 최종 확인 — 을 새로 조사한다.

방법론은 같은 조직의 `research-loop`가 세운 기준을 따른다: **README/랜딩페이지만 보고 결론 내지 않는다.** 아래 모든 코드 관련 서술은 `gh api`로 GitHub 저장소의 실제 파일(TypeScript/Python/Ruby/JSON)을 직접 페칭해 읽은 결과이고, 파일 경로·함수명·상수명을 인용 근거로 남긴다.

## 2. 이전 조사 재검증 (자기교정 포함)

### 2.1 semihcihan/App-Store-Optimization-CLI — "우리 같은 룰체크+de-anchored LLM 채점+held-out 게이트가 있는가?"

초기 결론: "없다." 이번엔 저장소 트리 302개 파일 전체를 받아 재검증했다.

- `grep -iE "openai|anthropic|gpt-|claude|llm"`을 저장소 전체 경로 목록에 돌리면 매치는 `website/src/pages/llms.txt.ts` 단 1건뿐이다. 이건 AI 크롤러가 사이트 요약을 읽도록 하는 `llms.txt` 규격 페이지이지 LLM API 통합이 아니다.
- `cli/services/prompts/aso-prompt-handler.ts`, `cli/dashboard-server/prompt-session.ts`라는 파일명만 보면 "LLM 프롬프트 관리 로직이 있다"고 오인하기 쉽다. 실제로 열어보면 `inquirer` 기반의 **CLI 대화형 입력**(Apple ID/비밀번호/2FA 코드 입력을 `inquirer.prompt(...)`로 받는 것)이었다 — `promptWithCliAsoPrompt()` 함수가 `apple_credentials`, `verification_code` 같은 케이스를 처리한다. LLM과 무관하다.
  **자기교정 지점**: 파일명·디렉토리명만으로 판단했다면 "prompt 관련 코드가 있으니 LLM 통합일 것"이라고 잘못 분류했을 뻔한 대목이다. 실제로 열어봐야 한다는 원칙을 이 저장소 자체가 증명한다.
- 실제 "평가" 로직은 `cli/mcp/services/aso-evaluate-keywords.ts`의 `handleAsoEvaluateKeywords()`인데, LLM 판정이 아니라 `minPopularity`/`maxDifficulty` **수치 임계값 필터**다(`DEFAULT_MIN_POPULARITY = 6`, `DEFAULT_MAX_DIFFICULTY = 70` 상수, `aso keywords` 서브프로세스 결과를 JSON 파싱해 컷오프만 적용). 루브릭도, 채점 모델도, 재생성도 없다.
- `cli/mcp/content/rules.md`는 App Store Connect 정책 요약(IAP 삭제 불가, 이모지 금지 등)을 담은 정적 마크다운으로 MCP 리소스로 노출될 뿐, 이 룰로 LLM이 채점하는 루프는 코드 어디에도 없다.

**결론(재검증 결과 = 최초 결론과 동일, 자기교정 없음)**: 이 저장소는 순수 결정론적 키워드 리서치/난이도 분석 CLI다. LLM 판정·재생성 루프가 코드베이스에 존재하지 않는다는 최초 결론이 맞았다.

**Attribution 재확인**: `cli/domain/keywords/policy.ts`의 `normalizeKeyword()`(`keyword.trim().toLowerCase()`), `sanitizeKeywords()`(정규화 후 `Set`으로 dedup)와 `cli/shared/aso-keyword-utils.ts`의 `normalizeTextForKeywordMatch()`(`.normalize("NFKC")` + `/[^\p{L}\p{N}\p{M}\s]/gu` 치환 + lowercase + 공백정리)를 직접 읽었다. aso-loop의 `checks.rs::normalize_keyword`/`sanitize_keywords`/`normalize_text_for_match`가 이 로직을 정확히 포팅했음을 코드 대 코드로 재확인했다(README §Open-source attribution의 서술과 100% 일치, `unicode-normalization` 크레이트 미사용으로 인한 근사치 차이도 README가 이미 정확히 밝히고 있음).

**부수 발견**: `cli/domain/keywords/limits.ts`의 `ASO_MAX_KEYWORDS = 100`은 **API 호출 1회당 허용 키워드 개수** 상한이지 Apple 키워드 필드의 글자수(100자) 제한과는 무관한 별개 상수다. 숫자가 우연히 같아 혼동하기 쉬운 지점이라 §3.4에서 다시 짚는다.

### 2.2 furkancingoz/aso-skill — "아키텍처가 우리와 얼마나 겹치는가"

초기 라운드는 라이선스(NOASSERTION)만 확인하고 코드는 참고하지 않았다. 이번엔 `agents/aso-full.md`, `agents/aso-quick.md`, `commands/aso-build.md`, `lib/keyword_engine.py`를 직접 읽었다.

- 구조: Claude Code 서브에이전트 3종(`aso-full`=opus 모델, `aso-quick`, `asc-api`) + 커맨드 8종(`/aso`, `/aso-build`, `/aso-connect`, `/aso-assets` 등) + Python 라이브러리(`keyword_engine.py`, `asc_api.py`, `rank_tracker.py`, `screenshot_composer.py`, `searchads_api.py`).
- `aso-full.md`의 4-Phase 워크플로(연구 → 최적화 → 런칭 → 개선)를 읽으면 **각 필드당 생성은 1회, 단일 패스**다. "Validation Checklist"는
  ```
  - [ ] Title ≤ 30 chars ✓
  - [ ] Subtitle ≤ 30 chars ✓
  ```
  형태로 되어 있는데, 이건 **LLM 자신에게 체크리스트를 셀프 마킹하도록 시키는 프롬프트 지시문**이다. aso-loop의 `checks.rs`처럼 실제로 문자열 길이를 세는 결정론적 코드가 아니다 — 이 저장소에는 애초에 생성물을 검증하는 코드(Rust/TS/Python 무엇으로도)가 없다. `lib/keyword_engine.py`의 `KeywordEngine`은 키워드 relevance/competition/priority를 분류하는 리서치 엔진이지, 생성된 카피를 채점하는 채점기가 아니다.
- 유일하게 "루프"에 가까운 것은 `aso-quick.md`의 `<iteration_protocol>`(파일 내 약 205~230행): 생성 후 사용자에게
  ```
  1. 🎨 Tone: [more professional | casual | premium | playful]
  2. 🎯 Focus: [specific feature to emphasize]
  3. 🔑 Keywords: [add/remove specific keywords]
  4. ✏️ Rewrite: [specific field to regenerate]
  5. 💾 Save: [store to memory and finish]
  ```
  메뉴를 보여주고, **사용자가 명시적으로 선택한 필드만** 재생성한다("Single field update: regenerate only that field"). 이건 사람이 트리거하는 수동 재작성이지, 목표 점수·종료조건·채점 모델을 갖춘 자동 self-improvement 루프가 아니다.
- `lib/keyword_engine.py`에 다음 하드코딩 상수가 있다: `APPLE_TITLE_LIMIT = 30`, `APPLE_SUBTITLE_LIMIT = 30`, `APPLE_KEYWORD_LIMIT = 100`. 이건 §3.4(Apple 글자수 불확실성)에서 "서드파티 SDK가 문자/바이트 구분 없이 100을 그냥 상수로 박아넣는 흔한 패턴"의 실측 사례로 재사용한다.

**결론**: 채점 루프가 코드 레벨에서 확인상 없음. aso-loop과 겹치는 지점은 "스토어 필드별 카피 생성"이라는 표면적 목표뿐이고, 결정론적 룰체크·LLM 루브릭·de-anchoring·held-out 게이트는 이 저장소 어디에도 없다 — **아키텍처 겹침 없음.**

### 2.3 fastlane/fastlane (deliver 모듈) — "글자수 하드코딩 상수 없음, App Store Connect API에 위임"

초기 결론을 이번엔 (a) 소스 재확인 (b) 공식 App Store Connect OpenAPI 스펙 전체를 내려받아 `maxLength` 전수 검색 (c) 실사용자 GitHub 이슈, 세 겹으로 검증했다.

- `deliver/lib/deliver/upload_metadata.rb`(827줄)를 grep한 결과, 숫자 리터럴은 `limit: 2`(재시도 횟수), `time_in_ms / 1000` 같은 글자수와 무관한 값뿐이다. 30/100/170/4000류 글자수 상수는 없다.
- `deliver/lib/assets/summary.html.erb`(업로드 결과 요약 HTML 템플릿)에서 "100"이 등장하는 유일한 지점은 `width: 100%`라는 CSS 값이다.
- GitHub 코드검색(`search/code`)으로 `fastlane/fastlane` 저장소 전체에서 `keywords`+`100` 조합을 검색해도 검증 로직에는 나타나지 않는다(리포트 템플릿 HTML 1건 제외).
- App Store Connect API의 공식 OpenAPI 스펙 미러(`EvanBacon/App-Store-Connect-OpenAPI-Spec`, `specs/latest.json`, 2026-06-23 갱신, 6.68MB, JSON Schema 정의 1,337개)를 직접 다운로드해 파이썬으로 전수 검색한 결과 **`maxLength` 문자열이 스펙 전체에서 0건**이었다. `AppStoreVersionLocalizationCreateRequest`/`UpdateRequest`의 `keywords` 속성, `AppInfoLocalizationCreateRequest`의 `name`(title)/`subtitle` 속성은 전부 `{"type": "string"}`이고 길이 제약이 스키마 레벨에 전혀 없다. 이전 라운드보다 검증 강도가 높아졌다(이전엔 개별 스키마 하나만 확인했다면, 이번엔 스펙 전체 6.68MB에서 매칭 0건임을 확인).
- fastlane GitHub 이슈 [#16226](https://github.com/fastlane/fastlane/issues/16226)("Add length and character checking to metadata precheck", closed)을 읽으면 실사용자가 `fastlane release` 업로드 단계에서 받은 실제 에러 메시지가 인용돼 있다:
  > "App Name must not contain control characters ... **Subtitle can't contain more than 30 characters.** Subtitle can't contain more than 30 characters. ..."

  이 메시지는 **fastlane 코드가 아니라 App Store Connect 서버가 업로드 시점에 반환**한 것이다(이슈 제목 자체가 "이런 위반은 이상적으로는 precheck 단계에서 미리 드러나야 한다"는 기능 요청이라는 사실이, 현재 fastlane에 클라이언트 사이드 사전 검증이 없음을 실사용자 리포트로 재확인해준다). 부수적으로 Apple 서버 에러 메시지 자체의 단어 선택이 "characters"임도 확인했다(§3.4에서 재사용).

**결론(자기교정 없음, 확신도 상승)**: 초기 조사가 정확했다. 이번엔 (i) 스펙 전체 grep과 (ii) 실사용자 이슈라는 두 개의 독립적 1차 자료로 신뢰도를 높였다.

## 3. 신규 조사

### 3.1 AI 카피 에이전트 계열: AppTweak Atlas AI, Jenova AI

상용 closed-source 제품이라 §2처럼 함수 단위 검증이 불가능하다. README/블로그가 아니라 "제품 공식 페이지"와 "AI/LLM 전용 공식 정보 페이지"(`llm-and-ai-info`, AI 크롤러가 참조하도록 만든 페이지 — 마케팅 문구보다 기술적 서술을 기대할 수 있는 소스)까지 함께 확인했다.

| 도구 | 확인된 내용 | 우리 아키텍처와 비교 |
|---|---|---|
| **AppTweak Atlas AI** | `atlas-ai` 페이지: "5~10개 설명 초안을 몇 분 안에 생성" 가능하다고 명시. "Relevancy Score(0~100)"로 서브타이틀·키워드를 채점하고 "50점짜리 서브타이틀을 90점으로 올리는 키워드 교체"를 제안한다고 설명. `llm-and-ai-info` 페이지(AI 전용 공식 정보)까지 확인했으나 "100+ 국가·10년+ 앱스토어/플레이 데이터로 학습된 AppTweak의 독점 AI 지능 계층"이라는 한 문장 외 기술 아키텍처 설명은 없었다. | 다중 초안(○, "5~10개") + 단일 스코어(Relevancy Score — 공개 자료상 단일 모델로 보이며, **다중 모델/다중 라운드 판정이라는 근거는 없음**) + 재생성 제안(○, 그러나 "제안"이지 "자동 재생성 루프"인지는 불명). de-anchoring·held-out 검증·reward-hacking 방지장치는 공개 문서로 확인도 반증도 불가능. |
| **Jenova AI** | "Product Copywriter" 에이전트가 "conversion-focused listing copy"(설명, 서브타이틀 변형, 스크린샷 캡션)를 생성한다는 마케팅 서술 확인. API 문서·기술 아키텍처 문서는 검색 범위 내에서 찾지 못함. | 생성 단계 존재만 확인됨. 채점/재생성/검증 단계의 존재 자체를 공개 자료로 확인도 반증도 못함. |

**정직한 한계**: closed-source 상용 도구는 §2의 OSS 3건처럼 함수·라인 단위로 검증할 수 없다. "홍보 문구 이상의 기술 근거를 찾지 못했다"는 사실 자체가, "없다"는 확정 증거가 아니라 "확인 불가"임을 명시해야 한다 — research-loop 문서가 강조한 "확인 안 됨"과 "존재하지 않음"의 구분을 그대로 적용한다.

### 3.2 OSS 에이전트 프레임워크(LangGraph/CrewAI)의 ASO 적용 사례

GitHub 저장소 검색(`aso+langgraph`, `aso+crewai`, `app-store-optimization+agent`)으로 조사했다. LangGraph 기반의 ASO 전용 대표 프로젝트는 유의미한 규모로 발견되지 않았다(범용 LangGraph awesome-list 저장소만 매칭). CrewAI 기반으로는 `Dilshan189/agentaso`(★1, 라이선스 없음)를 찾아 `main.py`를 직접 읽었다.

- 아키텍처: `Agent` 4개(`researcher`, `copywriter`, `marketing_strategist`, `ui_ux_designer`)를 `Crew(process=Process.sequential)`로 묶은 **완전 순차 단일 패스 파이프라인**이다. `research_task → writing_task → marketing_task → design_task` 순서로 각 태스크가 정확히 1회씩만 실행되고, 되돌아가는 엣지(리뷰 → 재작성)가 없다.
- `writing_task`의 `description`에 `"3 App Title Options (max 30 characters each)"`라는 문구가 있지만, 이는 **LLM에게 주는 프롬프트 지시문일 뿐**이다. `tools.py`에는 `ASOScraperTool` 하나만 정의돼 있고 생성물을 채점·검증하는 코드는 없다 — LLM이 30자를 넘겨도 잡아낼 방법이 없다.
- 비평/재작성 에이전트, 루브릭, 점수, 종료조건, held-out 검증 — 전부 없다.

**결론**: research-loop이 GPT Researcher/company-research-agent/MetaGPT의 소스에서 확인한 것과 정확히 같은 패턴("생성 단계들의 순차 파이프라인, 반박·재측정 없음")이 ASO 도메인의 CrewAI 구현에서도 동일하게 관찰된다. `Dilshan189/agentaso`는 ★1짜리 소규모 프로젝트라 대표성엔 한계가 있고(더 크고 성숙한 LangGraph/CrewAI 기반 ASO 전용 프로젝트는 이번 조사에서 발견하지 못했다), 검색 중 발견한 furkancingoz류의 "Claude/Cursor Agent Skill" 포맷 저장소들(`Eronred/aso-skills` ★1697 등, 별도 미검증)은 그래프형 프레임워크가 아니라 §2.2와 구조적으로 유사한 단일-패스 스킬이라 이번 3.2의 조사범위(그래프형 멀티에이전트 프레임워크)에서 제외했다 — 이 부분은 §5 백로그로 남긴다. "OSS 에이전트 프레임워크 생태계에 discourse/reward-hacking 방지 구조를 갖춘 ASO 파이프라인이 존재하는가"라는 질문에 대해, 이번에 찾은 유일한 직접 증거의 답은 **"없다"** 였다.

### 3.3 de-anchoring/LLM-as-judge의 reward hacking 방지 — 학술 근거 추가

`bizplan-loop`의 `DESIGN.md`(12개 항목, aso-loop README가 "동일 근거로 상속" 명시)가 이미 인용한 문헌 — TrustJudge, Rulers, Prometheus, "More Convincing Not More Correct", Meta-Rewarding, Length-Controlled AlpacaEval, "LLMs Cannot Self-Correct Reasoning Yet", Self-Refine, "Nine Judges Two Effective Votes", PoLL, MT-Bench, "Who Validates the Validators?", "Pairwise or Pointwise?" — 와 **중복되지 않는** 2편을 새로 찾았다.

- **[Scaling Laws for Reward Model Overoptimization](https://arxiv.org/abs/2210.10760)**(Gao, Schulman, Hilton; OpenAI, ICML 2023) — "gold-standard reward model"(사람 역할)이 라벨링한 데이터로 "proxy reward model"을 학습시키고, policy를 그 proxy에 대해 최적화하면서 **gold RM 점수와 proxy RM 점수가 어떻게 벌어지는지**를 정량 측정한 논문이다. RL 최적화든 best-of-n 샘플링이든, proxy에 대한 최적화가 계속되면 gold 기준 성능은 어느 지점부터 정체·역전되는데 proxy 점수는 계속 오른다 — 이게 바로 held-out gate가 탐지하려는 현상의 **원형(prototype)** 이다. aso-loop의 `--gate-model`은 이 논문의 "gold RM vs proxy RM" 구도를 "루프 참여 judge(proxy) vs 미참여 judge(gate)"로 근사한 구현이다.
  - **[불확실/한계, 자체 지적]**: Gao et al.의 gold RM은 (합성 실험 세팅이지만 개념상) "진짜" 선호를 대표하도록 설계된 기준이다. aso-loop의 `--gate-model`은 그저 **또 다른 LLM 프록시**일 뿐 gold reward가 아니다 — 두 프록시가 서로 다른 모델이라는 사실만으로 reward hacking이 "탐지"되는 게 아니라, "두 프록시 간 불일치가 관측"될 뿐이다. 이 구분은 `report.rs::write_loop_report`의 경고 문구("채점자 최적화(reward hacking) 의심")에 정확히 반영돼 있지 않다 — **엄밀히는 "채점자 간 불일치"이지 "실제 카피 품질과의 불일치를 직접 측정"한 게 아니므로, 표현이 다소 단정적/과장돼 있다.** (§4, §5에서 개선안 제시)
- **[Correlated Proxies: A New Definition and Improved Mitigation for Reward Hacking](https://arxiv.org/abs/2403.03185)**(Laidlaw, Singhal, Dragan; UC Berkeley, ICLR 2025) — reward hacking을 "reference policy가 겪는 state-action에서 proxy reward와 true reward의 상관계수 r"로 정식 정의하고, 최적화가 진행될수록 이 상관관계가 어떻게 붕괴하는지를 이론화한 논문이다. reference policy에 대한 KL 정규화보다 occupancy measure의 χ² divergence 정규화가 이론적으로 더 효과적임을 보였다.
  - 시사점: 현재 `loop_run.rs`는 매 회차 재생성 시 "이전 문서와 얼마나 달라졌는가"를 전혀 규제하지 않는다(길이 canary는 있지만 내용 드리프트 정규화는 없다). Correlated Proxies의 논리를 그대로 적용하면, judge 점수를 올리기 위해 문서가 매 회차 급격히 바뀔수록(=judge가 학습한 "그럴듯함" 패턴에 맞춰 드리프트할수록) 실제 품질과의 상관이 깨질 위험이 커진다 — **회차 간 유사도를 측정해 과도한 드리프트를 경고하는 지표**를 백로그로 추가할 근거가 된다(§5).
- **기존에 아는 것(FacTool, Loki)의 ASO 도메인 재적용**: FacTool(GAIR-NLP, [arXiv:2307.13528](https://arxiv.org/abs/2307.13528))의 핵심 원칙 — "LLM의 자기 판단이 아니라 도구를 이용한 실제 실행/조회로 검증한다" — 을 이번에 다시 짚어보니, **`checks.rs` 자체가 이미 이 원칙의 ASO 버전 구현체**라는 걸 재확인했다. 글자수(`chars().count()`), 키워드 커버리지(`normalize_text_for_match` 문자열 매칭), 금지어(`Regex::find`)는 전부 LLM 판정이 아니라 결정론적 코드로 처리된다 — 처음 설계할 때 FacTool을 참조해 언어화한 건 아니지만, 사후적으로 동일한 원칙이다. FacTool이 새로 시사하는 건 **아직 커버되지 않은 사각지대**다: README의 "Limitations" 항목이 이미 명시하듯 "`--brief`에 담긴 사실 주장이 틀렸다면 카피에 그대로 반영되고, 그 카피가 채점을 통과할 수 있다" — checks.rs는 이 문제를 전혀 다루지 않는다. Loki([arXiv:2410.01794](https://arxiv.org/abs/2410.01794))의 5단계 파이프라인 중 "check-worthiness 식별" 단계(모든 문장이 아니라 검증 가치가 있는 주장만 선별)는 이 사각지대를 저비용으로 메우는 데 쓸 수 있다 — 카피 전체가 아니라 **정량적·사실적 주장(예: "10만+ 다운로드", "1위/최초/업계 유일")만 정규식으로 1차 선별**한 뒤, 그 문장만 브리프와 대조하는 결정론적(혹은 룰+LLM 하이브리드) 2차 검사를 추가할 여지가 있다(§5).

### 3.4 App Store Connect 글자수 제한 — 최종 확인

이전 라운드: "공식 OpenAPI 스펙엔 maxLength 없음"까지 확인. 이번엔 (a) 스펙 전체 재검색(§2.3에서 이미 수행, 스펙 전체 0건 재확인 — 중복 조사 대신 상호 참조), (b) 서드파티 SDK의 하드코딩 상수 추가 수집, (c) Apple 공식 HTML 문서 접근 재시도를 진행했다.

- **OpenAPI 스펙**: §2.3에서 이미 확인 — `maxLength` 문자열이 스펙 전체(1,337개 스키마)에서 0건. keywords/title/subtitle에 해당하는 속성은 전부 제약 없는 plain string.
- **서드파티 하드코딩 상수, 2건째 확보**: `furkancingoz/aso-skill`의 `lib/keyword_engine.py`(§2.2)에 `APPLE_KEYWORD_LIMIT = 100`, `APPLE_TITLE_LIMIT = 30`, `APPLE_SUBTITLE_LIMIT = 30`이 문자/바이트 구분 주석 없이 파이썬 상수로 박혀 있다. `semihcihan/App-Store-Optimization-CLI`의 `ASO_MAX_KEYWORDS = 100`(§2.1)은 **글자수가 아니라 API 호출당 키워드 개수 제한**이라 착시에 주의해야 한다 — 서드파티 소스 2곳을 실제로 열어본 결과 "100이라는 숫자는 어디서나 등장하지만, 그게 문자 기준인지 바이트 기준인지 명시한 곳은 하나도 없다"는 게 재확인됐다.
- **Apple 공식 HTML 문서 접근 재시도**: `developer.apple.com/help/app-store-connect/...`, `.../documentation/appstoreconnectapi/app-store-connect-api-release-notes` 등을 WebFetch로 재시도했으나 **JS 렌더링 페이지라 정적 콘텐츠를 가져오지 못했다**(빈 본문만 반환). 이번 조사에서도 Apple 공식 페이지 원문을 직접 인용하는 데는 실패했다 — 이는 "확인 안 됨"이지 "존재하지 않음"이 아니다(브라우저 렌더링 가능한 도구가 필요, 이번 조사 툴셋의 한계로 기록 — §5 백로그).
- **간접 1차 자료(신규)**: fastlane GitHub 이슈 [#16226](https://github.com/fastlane/fastlane/issues/16226)에 인용된 **실제 App Store Connect 서버 에러 메시지** — `"Subtitle can't contain more than 30 characters."` — 서버 스스로 "characters" 단위로 자신을 지칭한다. 이건 이전 라운드가 인용한 "Apple Developer Forum 705360(태국어 100자 통과 사례)"보다 한 단계 더 공식적인 소스(실제 서버 응답 문자열)이지만, 여전히 **1차 공식 문서가 아니라 사용자가 캡처한 에러 메시지 인용**이라는 한계가 있다.
- **결론(갱신 없음, 확신도만 상승)**: "문자 기준일 가능성이 높으나(서버 에러 메시지의 단어 선택 + 태국어 100자 통과 사례 + OpenAPI 스펙에 바이트 제약 명시 없음), 100% 공식 1차 문서로 확정하지는 못했다"는 aso-loop README의 현재 서술은 **여전히 정확하다.** 이번 조사로 근거가 하나 더 늘었을 뿐(서버 에러 메시지의 "characters" 표현) 결론을 바꿀 반증은 없었다. README의 `[불확실]` 태그와 "App Store Connect에서 직접 확인" 권고는 그대로 유지하는 게 맞다.

## 4. 종합 결론

- **결정론적 룰체크 + LLM 루브릭 채점 자체는 흔하다.** furkancingoz/aso-skill도 "체크리스트"라는 형태로, AppTweak Atlas AI도 "Relevancy Score"라는 형태로 유사한 것을 표방한다. 이 부분만 놓고 "우리가 유일하다"고 주장하면 틀린 말이다.
- **차별점은 개별 요소가 아니라 조합과 엄격성이다.** (1) 재생성 전에 승리조건을 먼저 쓰게 하는 de-anchoring, (2) 원문 인용이 없으면 60점 상한, (3) 다중 모델·다중 라운드 절사평균, (4) 루프 미참여 모델의 held-out 재채점, (5) 길이 인플레이션 canary — 이 5개를 **전부 갖춘** 사례는 이번 조사(OSS 2건 재검증 + OSS 1건 신규 + 상용 2건 + 학술 조사)에서 하나도 발견되지 않았다. 특히 held-out gate는 상용 도구조차 공개 문서 수준에서 언급 자체가 없었다.
- **다만 이 결론은 "발견 못했다"이지 "존재하지 않는다"가 아니다.** 상용 도구(Atlas AI, Jenova AI)는 closed-source라 코드 레벨 반증이 원천적으로 불가능하고, LangGraph 진영은 ASO 전용 대표 프로젝트를 찾지 못해 CrewAI 표본이 사실상 1개(★1)뿐이었다 — research-loop이 company-research-agent 한 건으로 결론 낸 것과 비슷한 수준의 표본 한계를 그대로 안고 있다.
- **자기 회의적으로 봐야 할 지점 (신규 발견)**: `report.rs`의 held-out 경고 문구가 "reward hacking 의심"이라고 단정하는 건 §3.3에서 지적했듯 다소 과장이다. Gao et al.의 틀로 보면 aso-loop의 gate-model은 "gold reward"가 아니라 "또 다른 proxy"이므로, held-out 불일치는 "두 채점자 간 재현되지 않는 개선"의 신호이지 "실제 카피 품질 하락"을 직접 측정한 증거는 아니다. 이 구분을 README/report 문구에 더 명확히 반영할 가치가 있다.
- **`checks.rs`가 이미 FacTool류 도구증강 검증의 ASO 버전이라는 것**은 이번 조사로 새로 얻은 관점이다. 설계 당시엔 그런 의도로 언어화되지 않았지만, 사후적으로 보면 "LLM이 셀프 리포트하는 값(글자수·키워드 포함 여부·금지어)을 그대로 믿지 않고 코드로 재검증한다"는 원칙이 FacTool의 핵심 주장과 정확히 같다. 다만 **브리프-카피 사실정합성 검증은 이 원칙이 아직 적용되지 않은 사각지대**다(§3.3, §5).

## 5. 다음 단계 제안 (백로그)

우선순위 순.

1. **[P1] held-out 게이트 경고 문구 정정.** `report.rs::write_loop_report`의 "채점자 최적화(reward hacking) 의심" 문구를 "채점자 간 불일치(다른 프록시로는 재현되지 않는 개선)"처럼 더 정확한 표현으로 완화. README의 "held-out 게이트" 절에 Gao et al.(arXiv:2210.10760)/Correlated Proxies(arXiv:2403.03185) 인용과 "gate-model도 gold reward가 아니라 또 다른 proxy"라는 한계를 명시.
2. **[P2] 브리프-카피 사실정합성 검사 (FacTool/Loki 응용).** `checks.rs`에 "브리프에 없는 정량적 주장(다운로드 수, 순위, 수상 이력 등)이 카피에 등장하면 플래그"하는 검사를 추가. Loki의 check-worthiness 식별 단계를 참고해 우선 정규식/패턴(숫자+단위, "1위/최초/공식" 류)으로 결정론적 1차 필터링부터 시작하고, LLM 판정은 필터를 통과한 문장에만 적용해 비용을 통제.
3. **[P2] 회차 간 드리프트 지표 (Correlated Proxies 응용).** `loop_run.rs`에 회차 간 문서 유사도(예: 외부 크레이트 추가 없이 가능한 토큰 자카드 유사도)를 측정해 급격한 드리프트를 경고 — 길이 canary와는 별개로 "내용이 아예 다른 카피로 바뀌었는데 점수만 오른" 케이스를 탐지.
4. **[P3] Apple 문서 원문 확보 재시도.** 이번에도 JS 렌더링 페이지 접근 실패(WebFetch 한계)로 developer.apple.com 공식 HTML을 직접 인용하지 못했다 — 브라우저 자동화가 가능한 도구(예: Playwright MCP)로 재시도하면 `[불확실]` 태그를 없앨 근거를 확보할 수도 있다.
5. **[P3] 상용 도구 블랙박스 벤치마크.** closed-source라 코드 검증은 불가능하지만, Atlas AI/Jenova AI 무료 티어에 동일 브리프를 넣어 실제 산출물의 글자수 정확도·키워드 반영 품질을 블랙박스 테스트하면 "우리 대비 실사용 품질"을 실증적으로 비교할 수 있다(이번 조사는 문서 조사에 그쳤다).
6. **[P3] LangGraph/CrewAI 진영 재조사 주기화.** 이번엔 대표성 있는 프로젝트를 찾지 못했으나 생태계가 빠르게 변하는 영역이므로, 다음 라운드에서 "aso langgraph/crewai"류 검색을 반복해 새 진입자가 있는지 확인할 가치가 있다. 동시에 이번에 조사범위에서 제외한 "Claude/Cursor Agent Skill" 포맷 저장소들(`Eronred/aso-skills` ★1697 등, furkancingoz/aso-skill과 구조적으로 유사할 가능성이 높지만 미검증)도 언젠가 코드 레벨로 확인할 가치가 있다.

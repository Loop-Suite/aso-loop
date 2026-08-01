# aso-loop

앱스토어 리스팅 카피(제목/부제/키워드/설명)를 **여러 버전 생성 → 결정론적 룰체크 → 루브릭 채점 → 피드백 반영 재생성**하는 Rust CLI.
LLM 백엔드는 Claude Code CLI(`claude -p`) 서브프로세스. 별도 API 키 불필요.

[Loop-Suite/bizplan-loop](https://github.com/Loop-Suite/bizplan-loop)(같은 "N개 생성 → 룰체크 → LLM 루브릭 채점 → 재생성" 패턴의 사업계획서용 CLI)의
아키텍처를 그대로 가져와 ASO(App Store Optimization) 도메인으로 이식했다. `claude -p` 호출 방식, 스레드 분리 stdin/stdout 처리, JSON 스키마 강제,
de-anchoring/held-out gate 등 채점 메커니즘은 원본과 동일하다. 도메인 로직(스펙 필드, 결정론적 검사, 프롬프트)만 새로 작성했다.

## 요구사항

- Rust 1.70+
- `claude` CLI 설치 및 로그인 (PATH에 없으면 `--claude-bin`)

## 빌드

```bash
cargo build --release   # target/release/aso
```

## 3가지 모드

```bash
# 1) 초안 N개 생성 + 채점 + 랭킹
aso --model sonnet --judge-model haiku \
  gen --spec specs/example-apple.toml --brief brief.md -n 6 --rounds 2 --concurrency 3 --out runs/apple

# 2) 기존 리스팅 카피 채점만
aso --judge-model sonnet,haiku \
  score --spec specs/example-apple.toml --input listing.md --rounds 3 --out runs/check

# 3) 목표 점수까지 자기개선 루프 (+ held-out 검증)
aso --model opus --judge-model sonnet --gate-model haiku \
  loop --spec specs/example-google.toml --brief brief.md --target 85 --max-iter 4 --out runs/loop
```

`--brief`는 앱 개요·핵심 기능·타겟 유저·경쟁 앱 대비 차별점 등을 적은 md/txt 파일이다. 없는 사실을 적으면
모델이 그대로 카피에 반영하고, 그 부정확한 카피가 채점·검사를 통과할 수 있다.

## 백엔드 동작

`claude` CLI 호출 방식은 bizplan-loop과 완전히 동일하다(구조를 임의로 바꾸지 않았다).

```
claude -p --output-format json --safe-mode --no-session-persistence --tools "" \
       [--model M] [--append-system-prompt S] [--json-schema SCHEMA] [--max-budget-usd X]
```

| 플래그 | 이유 |
|---|---|
| `--safe-mode` | 실행 디렉터리의 CLAUDE.md·스킬·플러그인·훅·MCP를 로드하지 않음 → 재현성 확보. `--load-context`로 해제 |
| `--tools ""` | 내장 도구(Read/Edit/Write/Bash) 전면 차단 → 순수 텍스트 생성, 파일 접근 없음 |
| `--no-session-persistence` | 세션 파일 미생성. 병렬 실행 시 경합 회피 |
| `--json-schema` | 채점 결과를 스키마로 강제. 검증된 객체가 응답의 `structured_output`으로 옴 |
| `--output-format json` | `result` / `structured_output` / `total_cost_usd` 수집 |

프롬프트는 stdin으로 전달하고, stdin 쓰기와 stdout/stderr 읽기를 별도 스레드로 동시에 처리한다(파이프 버퍼 포화 교착 방지). 호출당 타임아웃은 `--timeout-secs`(기본 600).

## 채점 방식

1. **결정론적 검사**(`checks.rs`, LLM 미사용):
   - 필드별 글자수 초과/부족(스토어가 강제하는 하드 리밋 기준)
   - 필수 필드 누락
   - Apple의 title/subtitle/keywords처럼 스토어가 자동 dedup하는 필드 간 키워드 중복(글자수 낭비이므로 flag)
   - `target_keywords` 대비 실제 반영 키워드 커버리지
   - 금지어: 경쟁 앱명/상표명(스펙에서 정규식으로 지정) + 최상급 표현("best"/"#1"/"no.1"/"1위"/"최고" 등) + 가격·할인 문구 + 과도한 이모지(`emoji_max` 초과)
2. **LLM 루브릭 채점**: 항목별 0~100점. 채점 전에 "이 리스팅이 갖춰야 할 조건"을 먼저 쓰게 하고(de-anchoring), 항목마다 문서 원문 인용과 "왜 더 높은 점수가 아닌가"를 강제한다. 근거 인용을 못 하면 60점 상한. 기본 루브릭:

   | 항목 | 가중치 |
   |---|---|
   | keyword_relevance (키워드 적합성) | 0.30 |
   | conversion_copy (전환 카피) | 0.25 |
   | localization_quality (현지화 품질) | 0.15 |
   | readability_no_stuffing (가독성·비스터핑) | 0.15 |
   | compliance (정책 준수) | 0.15 |

3. **집계**: `--rounds N` 회 채점 → 모델·관점 순환 → 항목별 절사평균(n≥4면 최소·최대 제외) → 가중 합산.
4. **불안정 지표**: 항목별 점수 산포(±)를 리포트에 표시.
5. **held-out 게이트**(`--gate-model`): 루프에 참여하지 않은 모델로 최초본·최고본만 재채점. 루프 점수는 올랐는데 held-out 점수가 안 오르면 채점자 최적화(reward hacking)로 표시.

de-anchoring·절사평균·held-out gate·길이 canary 등 설계 근거는 bizplan-loop의 `DESIGN.md`(문헌 인용 포함)와 동일한 근거를 그대로 따른다.

## 스펙 (`specs/*.toml`)

```toml
name = "앱 이름 (스펙 설명)"
store = "apple"   # "apple" | "google"
context = "앱 개요·타겟 유저. 프롬프트에 그대로 삽입"
target_keywords = ["키워드1", "키워드2"]
banned_terms = ["경쟁앱명1", "경쟁앱명2"]   # 정규식, 대소문자 무시
emoji_max = 2

[[sections]]
id = "title"
title = "Title"
guide = "작성 지침"
max_chars = 30       # 스토어 하드 리밋
min_chars = 15        # 권장 최소(활용도 낮음 경고용)
required = true
keyword_dedup_target = true   # Apple title/subtitle/keywords만 true

[[criteria]]
id = "keyword_relevance"
name = "키워드 적합성"
weight = 30
guide = "..."
```

동봉 스펙:
- `specs/example-apple.toml` — title(≤30자)/subtitle(≤30자)/keywords(≤100)/promo_text(≤170, 선택)/description(≤4000자)
- `specs/example-google.toml` — title(≤30자)/short_description(≤80자)/long_description(≤4000자)

**Apple keywords 필드의 100자 제한이 문자 수 기준인지 바이트 기준인지는 불확실**하다(App Store Connect
공식 문서가 명확히 하지 않고, 한글 등 멀티바이트 문자에서 체감 차이가 날 수 있다는 보고가 있음). 이 프로젝트는
문자 수(`chars().count()`) 기준으로 계산하며, keywords 필드가 상한의 90%를 넘기면 리포트에 `[불확실]` 경고를
별도로 띄운다 — **실제 등록 전 App Store Connect에서 직접 확인 권장.**

## 오픈소스 출처

`src/checks.rs`의 `normalize_keyword` / `sanitize_keywords` / `normalize_text_for_match`는
[semihcihan/App-Store-Optimization-CLI](https://github.com/semihcihan/App-Store-Optimization-CLI)(MIT License)의
`cli/domain/keywords/policy.ts`(`normalizeKeyword`, `sanitizeKeywords`) 및
`cli/shared/aso-keyword-utils.ts`(`normalizeTextForKeywordMatch`) 로직을 참고해 Rust로 재작성한 것이다(코드
복사가 아니라 정규화·dedup 알고리즘만 포팅). 원본은 `.normalize("NFKC")` + 유니코드 정규식(`\p{L}\p{N}\p{M}`)을
쓰지만, 이 프로젝트는 `unicode-normalization` 크레이트를 추가하지 않고 `char::is_alphanumeric()` 기반으로
근사했다(완전한 NFKC 동등은 아님). 이 정규화 함수가 필드 간 키워드 중복 검사·타겟 키워드 커버리지 계산의 기반이다.

`furkancingoz/aso-skill`은 라이선스가 명시되지 않은 저장소(NOASSERTION)라 코드를 참고하지 않았다.

## 한계 · 가정

- LLM 점수는 실제 스토어 검색 랭킹이나 심사 통과를 보장하지 않는다. 같은 스펙·같은 채점 모델 안에서의 **상대 비교**와 **개선 방향 도출**용.
- 생성 모델과 채점 모델이 같으면 자기 문체를 후하게 본다(`--judge-model` 미지정 시 경고 출력).
- 키워드 커버리지 검사는 정규화된 텍스트에 타겟 키워드 문자열이 부분 일치(substring)로 포함되는지만 본다. 형태소 분석이나 어간 추출은 하지 않으므로 "가계부"를 "가계부는"처럼 활용형으로만 썼을 때는 잡히지만, 완전히 다른 단어(동의어)로 표현했을 때는 코드가 놓칠 수 있다.
- 금지어 검사는 스펙에 정의된 정규식과 기본 최상급/가격 패턴에 대한 것으로, 실제 앱스토어 심사 정책 전체를 커버하지 않는다.
- Apple keywords 필드 글자수 기준(자수 vs 바이트) 불확실 — 위 "오픈소스 출처" 절 참고.
- `claude -p`는 temperature를 노출하지 않는다 → 초안 다양성은 angle 프롬프트로만 만든다.
- 출력은 마크다운(`## 필드명` 헤딩). 실제 App Store Connect/Google Play Console 입력폼에 옮겨 붙이는 과정은 범위 밖.

## 다각도 리뷰 반영 내역

review-panel(functionality/good_things/tests 렌즈) 결과 CONFIRMED된 항목을 반영했다:
- 헤딩-필드 매칭을 영숫자+소문자 정규화 후 **정확 일치**로 변경(이전엔 양방향 substring 포함이라
  "Subtitle"이 "Title"을 문자 그대로 포함하는 식으로 오매칭될 수 있었음).
- `banned_terms`에 컴파일 불가능한 정규식이 있으면 `Spec::load` 시점에 에러(이전엔 조용히 무시돼
  해당 금지어 검사가 사라졌음).
- 필드 간 중복 키워드 검사에서 단일문자 제외 판정을 byte 길이 대신 char 개수로 수정(한글 1글자가
  안 걸러지던 버그).
- 채점 시 특정 criterion id에 한 번도 점수가 안 들어오면 경고 표시(이전엔 조용히 0점 처리).
- 병렬 실행 스레드 패닉이 전체 프로세스로 전파되지 않도록 처리.
- `banned_term` 검증을 실제로 하지 않던 테스트를 고치고 경계값(글자수 정확히 상한)·정규화 대소문자
  테스트를 추가.

## 스캐폴드 중 임의로 정한 부분

- CLI 플래그명 `--idea` 대신 `--brief`를 사용했다(사업계획서 "아이디어"보다 ASO 도메인에서는 "앱 개요 브리프"가 더 맞다고 판단).
- `min_chars`(권장 최소 글자수)는 스토어가 강제하는 값이 아니라 "활용도 낮음" 경고용으로 이 프로젝트가 임의로 도입한 개념이다. 예시 TOML의 값(예: title 15자 이상)도 임의로 정한 권장치이며 실제 상한 준수 의무는 `max_chars`만 해당한다.
- 이모지 임계치(`emoji_max` 기본 3, 예시 스펙은 2)와 금지어 기본 패턴(최상급 표현·가격 문구 목록)은 사용자 요구에 있는 "임계치 설정"을 구체적 숫자·패턴으로 확정한 것으로, 실제 스토어 정책 문서에서 가져온 수치가 아니다.

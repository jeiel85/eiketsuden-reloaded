<div align="center">

# 영걸전 Reloaded

**삼국지 영걸전(1995)을 현대 시스템에서 원작대로 돌리는 오픈소스 엔진**<br>
*An open-source engine that runs your copy of the classic Three Kingdoms tactics RPG "Sangokushi Eiketsuden" on modern systems*

[![CI](https://github.com/jeiel85/eiketsuden-reloaded/actions/workflows/ci.yml/badge.svg)](https://github.com/jeiel85/eiketsuden-reloaded/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/code-GPL--3.0--or--later-blue)](LICENSE)
[![Media: CC0 / CC-BY / OFL / PD](https://img.shields.io/badge/media-CC0%20%C2%B7%20CC--BY%20%C2%B7%20OFL%20%C2%B7%20PD-green)](CREDITS.md)

**[⬇ Windows / macOS / Linux 다운로드](https://github.com/jeiel85/eiketsuden-reloaded/releases)** ·
[소개 페이지](https://jeiel85.github.io/eiketsuden-reloaded/) ·
[English](#english)


</div>

## 소개

1995년 KOEI의 『삼국지 영걸전』은 유비의 일대기를 따라가는 턴제 전술 RPG였습니다. **영걸전 Reloaded**는
**사용자가 가진 원작**을 오늘날의 Windows · macOS · Linux에서 원작대로 플레이할 수 있도록 **엔진을 처음부터 새로 만든**
프로젝트입니다. [OpenRCT2](https://openrct2.io/)가 롤러코스터 타이쿤 2를, [OpenTTD](https://www.openttd.org/)가
트랜스포트 타이쿤을 되살린 것과 같은 방식입니다. 새 게임을 만드는 것이 아니라 원작을 재현하는 것이 목표이며, 그래서
원작 모드가 이 프로젝트의 본체입니다([DECISIONS D21](docs/DECISIONS.md)). 엔진을 어떻게 만들고 있는지
(OpenRCT2와 같은 점·다른 점, 원작 파일을 읽는 방법, 개발과 검증 방식)는 **[docs/ENGINE.md](docs/ENGINE.md)** 에 공개합니다.

* **원작 모드** — OpenRCT2처럼, 정품을 가진 사용자는 타이틀의 "원작 데이터"에서 자기 PC의 원작 설치 폴더를
  고르면 원작의 얼굴·유닛·지형·전투 맵과 화면 틀(전투·캠프·상태 창), 음악, 병종·지형·책략·회복 아이템·혼란 규칙 표로
  플레이할 수 있습니다. 서장부터 원작의 엔딩까지 전투 62개와 그 대사·일기토, 마을 이야기(대사 장면과 선택지),
  무장의 능력치와 합류가 사용자의 원작 파일에서 변환됩니다(네이티브 빌드, 명령줄 불필요, 원작 파일은 읽기만 함).
  서장·1장의 원작 변환은 새로 들어가 실물 원작으로 아직 확인하지 않았습니다.
  [docs/ORIGINAL_DATA.md](docs/ORIGINAL_DATA.md)
* **원작 규칙 재현** — PC판(국내 DOS판)의 규칙을 팬 커뮤니티가 역분석한 공식 그대로 옮겼습니다.
  공격력·방어력 `(Lv+10)×(사기/10 + 400/(140−능력치) + 병종 보정)`, 확정 데미지 `(공격 − 방어/2)×지형`,
  병종 상성(방어 ±25%), 무력/150 확률의 반격, 사기와 혼란, 책략 명중·위력 공식, 날씨(비 오면 화계 불가) 등.
  자세한 내용은 [docs/RULES.md](docs/RULES.md).
* **라이선스 청정 배포물** — 저장소와 배포물에는 원작의 그래픽·음악·대사·실행 파일이 한 바이트도 들어 있지 않습니다.
  원작은 사용자의 PC에서만 읽습니다.
* **기본 팩 (엔진의 최소 틀)** — 규칙 기본값, 원작 무장과 이름으로 짝짓는 무장 명단, 엔진이 쓰는 라이선스 청정 그림·음악·
  폰트·효과음(CC0 에셋·자체 제작·퍼블릭 도메인 고화·CC-BY/CC0 곡, [CREDITS.md](CREDITS.md)), 그리고 원작 없이 엔진을
  시험해 보는 **사수관 전투 하나**만 담습니다. 이야기와 캠페인은 원작 모드가 사용자의 원작에서 만듭니다(D21).
* **모딩** — 규칙·무장·전투 맵·캠페인·대사가 모두 사람이 읽을 수 있는 TOML과 `.drama` 스크립트입니다.
  레이어드 팩(`extends`)으로 기본 팩 위에 바꿀 파일만 담은 모드를 만들 수 있습니다. [docs/MODDING.md](docs/MODDING.md)


## 플레이하기

원작으로 플레이하려면 **데스크톱 빌드**(Windows · macOS · Linux)를 받아 실행한 뒤, 타이틀의 **원작 데이터**에서 가지고 있는
원작의 설치 폴더를 고르세요. 다음 실행부터는 바로 원작 모드로 시작합니다. 지금 고를 수 있는 판본은 한국어 DOS/V판과 번체 중문 DOS판이며,
실물로 검증한 것은 한국어 DOS/V판입니다([docs/ORIGINAL_DATA.md](docs/ORIGINAL_DATA.md) 2절). 원작 폴더를 고르지 않으면 기본 팩의 시험 전투(사수관)로 시작합니다.

| 방법 | |
|---|---|
| Windows | [Releases](https://github.com/jeiel85/eiketsuden-reloaded/releases)에서 `…-windows-x64.zip`을 받아 압축을 풀고 `eiketsuden.exe` 실행 (`data` 폴더를 실행 파일 옆에 그대로 두세요) |
| macOS | `…-macos-arm64.tar.gz`(Apple Silicon) 또는 `…-macos-x64.tar.gz`(Intel)를 풀고 터미널에서 `./eiketsuden` 실행. Apple 공증을 받지 않은 실행 파일이라 처음 실행이 막히면 **시스템 설정 → 개인정보 보호 및 보안**에서 '그래도 열기'를 누르거나, 터미널에서 `xattr -dr com.apple.quarantine <압축을 푼 폴더>` 후 다시 실행하세요. (Finder에서 우클릭 → 열기로 넘기는 방법은 macOS 14 이하에서만 됩니다.) |
| Linux | `…-linux-x64.tar.gz`를 풀고 `./eiketsuden` 실행 |

### 조작

| 동작 | 키보드 | 마우스 / 터치 |
|---|---|---|
| 결정 | Z · Enter · Space | 왼쪽 클릭 · 탭 |
| 취소 / 메뉴 | X · Esc · Backspace | 오른쪽 클릭 |
| 커서 이동 | 방향키 · WASD | 마우스 이동 · 드래그로 화면 이동 |
| 행동 가능한 부대 순환 | Tab · Q · E | — |
| 대사 빨리 넘기기 / 최근 대사 | Ctrl · Tab 누르고 있기 / L | 화면 오른쪽 위 버튼 |
| 순간 저장 / 순간 불러오기 | F5 / F9 | 대사 화면의 메뉴(X · 오른쪽 클릭)와 전투 메뉴의 '순간 저장' · '순간 불러오기' |
| 전체 화면 (데스크톱) | F11 · Alt+Enter | — |

**순간 저장**은 에뮬레이터의 순간 저장처럼 대화 중, 선택지 앞, 전투 연출과 적 턴 도중에도 바로 저장하고 이어 할 수 있습니다.
기록 칸 하나를 따로 쓰므로 자동 기록과 기록 1~8을 덮어쓰지 않으며, 타이틀의 '이어하기'는 가장 최근 기록(순간 저장 포함)을 불러옵니다.

## 직접 빌드하기

Rust(stable, 1.85+)가 필요합니다. Linux에서는 `libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev`도 설치하세요.

```bash
cargo run --release -p hero-game
```

웹 빌드(WebAssembly)는 `rustup target add wasm32-unknown-unknown` 후(브라우저는 로컬 원작 폴더를 읽을 수 없어 웹에는 원작 모드가
없고 기본 팩의 시험 전투만 돕니다. 웹 데모는 배포하지 않고, GitHub Pages에는 `site/`의 소개 페이지만 올립니다, D21):

```bash
tools/web/build.sh --serve 8080
```

Windows PowerShell에서는 `pwsh tools/web/build.ps1 -Serve 8080`. 자세한 내용은 [docs/DEVELOPING.md](docs/DEVELOPING.md).

데이터 팩 도구:

```bash
cargo run --release -p hero-tools -- validate data/base
```

`simulate <팩>`은 팩의 모든 전투를 AI 대 AI로 돌려 봅니다. `--campaign`을 붙이면 캠페인을 처음부터 따라가며
앞 전투의 군대를 이어받습니다.

## 구조

| 경로 | 내용 |
|---|---|
| `crates/hero-core` | 규칙, 전투 엔진과 AI, 데이터 팩 로딩·검증, 캠페인, 드라마 스크립트, 세이브 (그래픽·OS 의존 없음) |
| `crates/hero-game` | 게임 실행 파일 (macroquad, 네이티브 + WebAssembly) |
| `crates/hero-tools` | `validate` · `simulate` · `info` · `original` 명령줄 도구 |
| `crates/hero-import` | 원작 데이터 임포터 (실험적, 클린룸 구현) |
| `data/base` | 기본 데이터 팩(엔진의 최소 틀): 규칙, 무장 명단, 그래픽, 음악, 폰트, 효과음, 시험 전투 하나 |
| `tools/assets` | 에셋 파이프라인 (출처 URL·SHA-256 고정, 결정적 빌드) |

설계 문서: [ENGINE(개발 방식)](docs/ENGINE.md) · [ARCHITECTURE](docs/ARCHITECTURE.md) · [RULES](docs/RULES.md) · [MODDING](docs/MODDING.md) ·
[ASSETS](docs/ASSETS.md) · [DECISIONS](docs/DECISIONS.md) · [ORIGINAL_DATA](docs/ORIGINAL_DATA.md) ·
[원작 데이터 분석 자료](docs/reverse-engineering/README.md)

## 진행 상황

진행률은 **원작 모드**만 기준으로 잽니다(D21). 원작의 전투 62개(시나리오 5개 파일의 전투 블록, 루트마다 다른 전투
포함)를 모두 원작 모드로 플레이할 수 있고, 캠페인이 원작의 엔딩까지 이어집니다. 다만 마을은 걸어 다니는 대신 대사
장면과 선택지로 옮겼고, 서장·1장의 변환은 실물 원작으로 아직 확인하지 않았으며, 아래 "남은 일"처럼 간략하게 옮긴
부분이 있습니다. 기준과 세부는 아래 표에 있고, 기능이 머지될 때마다 갱신합니다.

| 영역 | 기준 | 진행 |
|---|---|---|
| 원작 모드 전투 | 원작 전투 62개 중 원작 데이터로 다시 짜거나 만든 것 | `███████████████` 62/62 (100%) — 서장·1장 19개(실물 변환 확인, 플레이 확인 전), 2장 10개, 3장 20개, 4장 13개 (두 맵에 걸친 장판파·와구관은 전투 둘로 나뉘고, 장판파는 백성 호위 포함. 4장 마지막 두 전투는 v0.5.0에서 추가, 이슈 #95) |
| 원작 모드 캠페인 | 시나리오 파일 5개(서장·1·2·3·4장) | `███████████████` 5/5 (100%) — 서장부터 원작 이야기를 변환한 원작 캠페인(엔딩 4개, 서장·1장은 실물 변환 확인, 플레이 확인 전) |
| 기본 팩 콘텐츠에서 독립 | 원작 모드가 기본 팩의 이야기·전투 뼈대·무장 데이터 없이 원작 파일만으로 서는 것 | `███████████████` 완료(실물 변환 확인, 플레이 확인 전) — 이야기·전투·캠페인, 무장(능력치·병종·레벨·장비, 합류하는 원작 무장), 일반 유닛의 능력치가 원작에서 옴. 기본 팩은 최소 틀(규칙 기본값·무장 명단·그림·음악·폰트·시험 전투)로 줄임(D21 4단계). 기본 팩에서 오는 것: 시작 무장 목록, 규칙 파일의 틀, 원작에서 변환하지 않는 그림·소리 |
| 원작 모드 변환 단계 | [STATUS 4절](docs/reverse-engineering/STATUS.md)의 필수 5단계 | 모두 **부분**: 원작 모드 팩·시나리오 변환(서장~4장)·규칙 표(병종·지형·책략·아이템·사기)·원작 UI(전투 틀·캠프·상태 창)·음악(곡 배정은 들어 보기 전 추론) |
| 파일 형식 해독 | [STATUS 1절](docs/reverse-engineering/STATUS.md)의 추출 영역 | 주요 형식 모두 추출. 남음: 명령 일부의 의미, 오프닝·엔딩 코덱, 세이브 |

원작 모드의 남은 일:
전투 이벤트 스크립트의 삽화, 음악 곡 배정 확인과 루프 시작점, 직접 플레이로
난이도 확인([BACKLOG](BACKLOG.md)). 기본 팩의 제2장 이후 캠페인은 만들지 않습니다(D21).

## 로드맵

* 서장·1장 원작 변환과 원작 무장 값의 플레이 확인(실물 변환 확인은 끝남, D21 3단계는 들어감, 기본 팩은 최소 틀로 줄였고 웹 데모는 내림)
* 원작 모드 다듬기: 위 "원작 모드의 남은 일"([BACKLOG](BACKLOG.md), 순서와 구현 계획은 [ROADMAP](docs/ROADMAP.md))
* 원작 데이터 임포터: Steam판·PC-98판 지원 ([남은 과제](docs/reverse-engineering/STATUS.md); 정품 보유자의 [프로브 매니페스트](docs/ORIGINAL_DATA.md) 제공이 큰 도움이 됩니다)
* 캠페인 경로를 따라가는 밸런스 시뮬레이션
* 원작에 없는 선택 기능([D25](docs/DECISIONS.md), 기본값은 언제나 원작 그대로): 새 게임의 난이도(쉬움·기본·어려움)·능력치 자유 조정·확장 규칙(협공), 설정 화면의 얼굴(원작/공개 초상화)·위험 범위 표시·전투 연출(빨간 `-피해` 숫자와 큰 타격의 화면 흔들림)

## 원작 데이터 분석 자료

원작 데이터 임포터를 만들며 알아낸 파일 형식과 분석 방법을 [docs/reverse-engineering/](docs/reverse-engineering/README.md)에
정리했습니다. 보유한 한국어 DOS/V판 하나를 v0.2.1까지는 **읽기 전용·정적 분석**으로만 조사했고, 그 뒤로는 사본을 공식
DOSBox-X에서 관찰용으로만 실행해 확인합니다(복제 방지는 우회하지 않음, D22). 저장소에는 원작 바이트나 데이터 표 없이 형식 사실만 적었습니다.

| 문서 | 내용 |
|---|---|
| [FORMATS](docs/reverse-engineering/FORMATS.md) | 판본 식별, LS11 컨테이너, TF-DCE 얼굴 압축, 팔레트, 스프라이트, 맵, 대사, 시나리오 바이트코드, `BAKDATA`의 형식 명세(항목마다 신뢰도 표기) |
| [METHOD](docs/reverse-engineering/METHOD.md) | 작업 순서와 기법, 함정, 공명전·조조전 등 다른 KOEI 게임에 적용할 체크리스트 |
| [SCENARIO](docs/reverse-engineering/SCENARIO.md) | 시나리오의 흐름(블록·선택지·질문·블록 이동·합류와 이탈·전투 블록)과 장별 구조. 블록 단위 개요는 [SCENARIO_FLOW](docs/reverse-engineering/SCENARIO_FLOW.md), 원문이 든 흐름은 `hero-tools original extract`가 사용자 컴퓨터에 만듦 |
| [STATUS](docs/reverse-engineering/STATUS.md) | 해독한 것·남은 것, 플레이 가능한 원작 모드까지 남은 단계 |

## 라이선스 · 크레딧 · 고지

* 코드: [GPL-3.0-or-later](LICENSE)
* 텍스트 콘텐츠(시나리오·대사): CC-BY-SA-4.0
* 미디어: 각 원작자의 라이선스(CC0 / CC-BY / OFL / 퍼블릭 도메인) — [CREDITS.md](CREDITS.md)

> **KOEI TECMO와 관계없는 독립 팬 프로젝트입니다.** KOEI TECMO GAMES가 만들거나 승인·지원하는 게임이 아닙니다.
> 이 저장소와 배포물에는 원작의 그래픽·음악·효과음·텍스트·데이터·코드가 **들어 있지 않습니다.**
> "영걸전(英傑伝)"이라는 이름은 이 프로젝트가 어떤 게임을 재구현하는지 밝히기 위해서만 씁니다.
> 『삼국지』, 『삼국지 영걸전』과 KOEI TECMO는 각 권리자의 상표입니다. 원작의 규칙과 수치는 사실 정보로서 재현했습니다.
> 결정 근거는 [DECISIONS.md](docs/DECISIONS.md)(D3, D5)에 있습니다.

---

## English

**Eiketsuden Reloaded** is an open-source engine that runs **your own copy** of KOEI's 1995 tactical RPG
*Sangokushi Eiketsuden* (Romance of the Three Kingdoms: Eiketsuden) on modern Windows, macOS and Linux, in the
spirit of OpenRCT2 and OpenTTD: a new engine written from scratch in Rust with
[macroquad](https://github.com/not-fl3/macroquad). The goal is to reproduce the original game, not to make a new
one, so the "original mode" is the heart of the project (decision D21 in [docs/DECISIONS.md](docs/DECISIONS.md)).

* **Original mode**: pick your installation folder under "원작 데이터" on the title screen (native builds) and the
  game converts your copy at every launch and plays it with the original's portraits, units, terrain, battle maps,
  screen frames, music and rule tables. All 62 of the original's battles, their dialogue and duels, and the story
  (towns become dialogue scenes with choices) are converted from your files, from the prologue to the original's
  endings, and so are the officers' stats and the persons who join. The conversion of the prologue and chapter 1 is new:
  it is checked on a real copy but not yet play-tested. Supported: the Korean DOS/V release (verified) and the
  Traditional Chinese DOS release (not yet verified on a real copy) — see [docs/ORIGINAL_DATA.md](docs/ORIGINAL_DATA.md).
* Faithful PC-version rules (reverse-engineered formulas for attack/defense, deterministic damage, class
  affinity, counters, morale and confusion, strategies, weather) — see [docs/RULES.md](docs/RULES.md).
* License-clean distribution: no KOEI graphics, music, text or code is in this repository or its releases; the
  original is only read on the player's own computer.
* **Base pack** (the engine's minimal frame): default rules, the officer list the original's persons are
  matched to, the license-clean art, music, fonts and sounds the engine uses (CC0 pixel art, public-domain
  paintings, CC-BY/CC0 music, [CREDITS.md](CREDITS.md)) and **one test battle** (Sishui Pass) to try the engine
  without the original. The story and campaign come from the player's copy (D21). There is no web demo: a
  browser cannot read a local installation folder. The [project page](https://jeiel85.github.io/eiketsuden-reloaded/)
  is a static introduction (`site/`).
* A fully data-driven, moddable format ([docs/MODDING.md](docs/MODDING.md)); a layered pack (`extends`) holds
  only the files a mod changes.
* How the engine is built — the OpenRCT2 model without reverse-engineering the original's code: a new engine
  from the published rules, license-clean data packs, and the original game read from the player's own copy
  and converted at launch (native builds) — is described in [docs/ENGINE.md](docs/ENGINE.md) (Korean,
  English summary).
* Reverse-engineering notes: the verified file formats of the original DOS/V release, the method used
  (read-only static analysis of an owned copy up to v0.2.1; since then a copy is also run for observation
  only in the official DOSBox-X, copy protection never bypassed, decision D22) and the open work are documented in
  [docs/reverse-engineering/](docs/reverse-engineering/README.md).
* Progress is measured on the original mode only: all 62 of the original's battles are playable, from the
  prologue to the original's endings (Changban's escort of the people and the battles fought on two maps are
  included). What remains is play-testing the prologue and chapter 1 and the polish listed in the Korean
  "진행 상황" section above.

**Download:**
[Releases](https://github.com/jeiel85/eiketsuden-reloaded/releases) · **Build:** `cargo run --release -p hero-game`

On macOS the release executable is not notarized: if the first start is blocked, click **Open Anyway**
in System Settings → Privacy & Security, or run `xattr -dr com.apple.quarantine <unpacked folder>` in
Terminal and start `./eiketsuden` again (right-click → Open only bypasses Gatekeeper up to macOS 14).

The game text is currently Korean only.

> **Not affiliated with KOEI TECMO.** This is an independent fan project; it is not made, endorsed or supported by
> KOEI TECMO GAMES CO., LTD. No material of the original game (graphics, music, sound, text, data or code) is
> included in this repository or its releases. The name "Eiketsuden" is used only to say which game this project
> reimplements; *Sangokushi*, *Sangokushi Eiketsuden* and KOEI TECMO are trademarks of their respective owners.

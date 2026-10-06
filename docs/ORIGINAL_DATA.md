# 원작 데이터 가져오기 (실험적)

> **English summary** — see [the end of this page](#english-summary).

영걸전 Reloaded는 자체 제작한 라이선스 청정 기본 팩(`data/base`)만으로 완전히 동작합니다. 이 문서가 설명하는
**원작 데이터 임포터**는 KOEI의 1995년작 『삼국지 영걸전』 정품을 **직접 보유한 플레이어**가 자기 PC에서
원작 파일을 읽어 중립 형식(PNG, UTF-8 JSON)으로 바꿔 쓰게 해 주는 **선택적·실험적** 기능입니다.
OpenRCT2가 사용자의 RCT2 데이터를 읽는 방식과 같습니다.

| 구성 요소 | 위치 |
|---|---|
| 라이브러리 | `crates/hero-import` (프로브·컨테이너·텍스트·그래픽·추출·원작 모드 팩) |
| 명령줄 | `hero-tools original probe` / `extract` / `pack` |
| 게임 연동 | **원작 모드**: 타이틀의 "원작 데이터"에서 설치 폴더를 고르면 실행할 때마다 메모리에서 변환해 플레이(4.1절, 네이티브 전용). 개발·검증용: 파일로 쓴 원작 모드 팩 `eiketsuden --data data/original`(4.5절), 미디어 오버레이 `eiketsuden --original <폴더>` 또는 환경 변수 `EIKETSUDEN_ORIGINAL` |
| 분석 자료 | [reverse-engineering/](reverse-engineering/README.md) — 형식 명세([FORMATS](reverse-engineering/FORMATS.md))·방법론([METHOD](reverse-engineering/METHOD.md))·현황([STATUS](reverse-engineering/STATUS.md)) |

## 1. 법적·윤리적 원칙

* **정품을 직접 보유한 경우에만** 쓰세요. 이 프로젝트는 원작 파일·디스크 이미지·ROM을 배포하지도, 내려받는
  방법을 안내하지도 않습니다.
* **읽기 전용·로컬 전용**입니다. 임포터는 설치 폴더에 아무것도 쓰지 않고(출력 폴더가 설치 폴더 안이면
  거부합니다), 아무것도 업로드하지 않습니다. 결과는 사용자가 고른 로컬 폴더에만 저장됩니다. 저장소 안에
  두려면 `.gitignore`에 등록된 `data/original/`을 쓰세요. 기본 출력 위치는 없습니다(`--out`은 필수).
* **복제 방지(copy protection)를 우회하지 않습니다.** 컨테이너가 암호화되어 있으면 감지·보고에서 멈춥니다.
  (Steam판 컨테이너 형식은 아직 모릅니다. 암호화가 확인되면 복호화기를 만들지 않고 법률 검토를 먼저 합니다.)
* **클린룸 구현**입니다. 공개된 형식 *사실*(이름·크기·구조·수치)만 조사 노트로 정리해 구현했고, 다른 프로젝트의
  코드(OpenEiketsuden — 라이선스 없음, ccz-compat-engine — GPLv3, lemonhall/LS11_encode_decode — 라이선스
  없음)는 읽거나 복사하지 않았습니다. 분석은 보유한 사본을 읽기 전용으로 정적으로 한 것이고,
  v0.2.1 이후에는 그 사본의 복사본을 공식 DOSBox-X에서 관찰용으로만 실행해 확인합니다(복제 방지는 우회하지 않음,
  DECISIONS D22; 원칙과 방법: [reverse-engineering/README.md](reverse-engineering/README.md)). 임포터와 게임은 원작을 실행하지 않습니다.
* **저장소와 CI에는 원작 바이트가 한 바이트도 없습니다.** 모든 테스트는 우리 인코더(LS11 인코더 포함)로 만든
  합성 데이터를 씁니다. 실제 설치본으로 하는 골든 테스트는 개발자가 환경 변수로 자기 설치본을 가리킬 때만
  돕니다.
* **추출 결과물은 공유하지 마세요.** 추출된 PNG·텍스트는 KOEI TECMO의 저작물입니다. 공유해도 되는 것은
  내용이 없는 **프로브 매니페스트**뿐입니다(5절).

## 2. 지원 판본

판본은 파일 이름·크기·헤더로 식별하며, 어느 규칙에도 맞지 않으면 추측하지 않고 `unknown`으로 보고합니다.
판정에는 항상 근거(evidence)가 함께 기록됩니다.

| 판본 | 식별 규칙 | 식별 | 추출 |
|---|---|---|---|
| 한국어 DOS/V (비스코, `korean-dos`) | `DISK1.R3I`의 **일본어 DOS/V 디스크 헤더**(Shift-JIS `DOS/V 三國志英傑伝`) + DOS/V 파일군 + 메시지 파일이 한국어 EUC-KR(2바이트 쌍의 80 % 이상이 한글 영역, 한글 500자 이상). 조사 노트가 말한 EUC-KR 헤더 `DOS/V 삼국지영걸전`도 대체 규칙으로 유지(실물에서는 못 봄) | 높음 | **지원** (텍스트·이름·스프라이트·얼굴·맵) |
| 번체 중문 DOS (第三波, `chinese-dos`) | DOS/V 파일군 + `SNRnM`/`IPPAN0M`/`BAKDATA`의 텍스트가 뚜렷한 Big5 | 중간 (통계적 판정) | **지원** (텍스트·이름·스프라이트·얼굴·맵, 실기 검증 전) |
| Steam 2017 (`steam-2017`) | `Eiketsuden1_Launcher.exe` 존재 | 높음 | **미지원** — 컨테이너 형식 미상, 매니페스트 수집 중 |
| PC-98 디스크 이미지 (`pc98-disk-images`) | D88 / Anex86 FDI·HDI 헤더 + 크기 일관성 | 높음 | **미지원** — 이미지 안의 파일 읽기는 P8 |
| 그 밖 | — | `unknown` | 거부 (`--edition`으로 강제 가능, 기록됨) |

검증한 한국어판 사본은 일본어 DOS/V판 위에 만든 현지화판입니다. `DISK1.R3I`에는 일본어판 헤더
(`DOS/V 三國志英傑伝 ﾃﾞｨｽｸ1 Ver 1.00 Rel 1.00`, Shift-JIS, `(C)(P) 1995 KOEI`)가 그대로 있고, 대사 파일만
EUC-KR 한글입니다. 그래서 헤더만으로는 언어를 정하지 않고 텍스트로 정합니다. 일본어 헤더에 한국어가 아닌
텍스트가 든 폴더(일본어 DOS/V판 — 본 적 없음)는 `unknown`으로 남깁니다.

식별은 지정한 폴더 바로 아래 파일만 봅니다. 한국어 DOS/V판은 설치 폴더 안의 `GAME` 폴더를 지정하세요
(`GAME` 하위 폴더가 보이면 프로브가 힌트를 줍니다).

## 3. 에셋 종류별 신뢰도와 현재 상태

| 에셋 | 원작 파일 | 상태 | 신뢰도 / 남은 가정 |
|---|---|---|---|
| LS11 아카이브 | 대부분의 `.R3` | 구현, **실물 검증** | 높음. `LS11`(디렉터리 빅엔디언)과 `Ls11`(리틀엔디언, `OPGRP`/`END1GRP`/`END2GRP`) 모두. 디렉터리 체인, 마지막 항목이 파일 끝에서 끝남, 정확한 복원 길이, 입력 완전 소비를 모두 검사. 한국어판 24개 중 23개가 전부 통과, `OPGRP.R3`는 검증한 사본이 손상([FORMATS §4.3](reverse-engineering/FORMATS.md#ls11)). `Ls10`/`Ls12` 변형은 "지원 안 함"으로 보고(실물에는 없음) |
| 6바이트 테이블 컨테이너 | `FACEDAT.R3`, `PACKGRP.R3` | 구현, **실물 검증** | 높음. 오프셋은 데이터 영역 기준(0번 = 0), 항목 수는 파일에 없어 체인이 파일 끝에서 끝나는 유일한 수로 구함([FORMATS §5](reverse-engineering/FORMATS.md#table6)) |
| 대사·문자열·시나리오 | `SNR0M`–`SNR4M.R3`, `SNR0D`–`SNR4D.R3`, `IPPAN0.R3`, `IPPAN0M.R3` | **추출** → `text/snr<n>.json`·`.txt`, `townsfolk_talk.json`, **실물 검증** | 높음. 바이트코드를 해독해 대화(화자별 줄)와 문자열의 경계를 정하고, 모든 섹션의 모든 바이트가 덮이는지 검사([FORMATS §11](reverse-engineering/FORMATS.md#text), [FORMATS §13](reverse-engineering/FORMATS.md#scenario)). 명령 일부는 이름·의미 미확인. 깨끗하게 디코딩되지 않은 텍스트는 원본 hex를 함께 기록 |
| 팔레트 | `MAIN.EXE` 안 | **추출** → `gfx/original/palettes.json`, **실물 검증** | 높음. 9 슬롯 × 48바이트 + `80 40 20 10` 서명으로 위치 탐색(고정 오프셋 안 씀), [B][R][G] 4비트. 한국어판은 0x38DF0. 게임은 **슬롯을 실행 중에** 시나리오·맵 데이터로 고르므로, 추출기는 눈으로 확인한 슬롯을 아카이브별로 씀([FORMATS §7](reverse-engineering/FORMATS.md#palette)) |
| 유닛 스프라이트·맵 칩·전투 UI 아이콘 | `HEXBCHR`, `HEXICHR`, `HEXZCHR`, `HEXZCHP`, `HEXBCHP`, `MMAPBGPL`, `SMAPBGPL`, `HEXGRP`(0번) | **추출** → `gfx/original/<파일>/<nnn>.png`, `sheets/<파일>.png`, `sprites.json`, **실물 검증(눈으로)** | 높음. 16×16 셀·4 비트플레인·MSB=왼쪽, 플레인 p = 색 비트 p, 셀은 행 우선 — 모두 실물 렌더링으로 확인. 항목 크기별 배치와 항목 묶음(병종·효과)은 [FORMATS §8](reverse-engineering/FORMATS.md#planar). 저장된 한 방향만 내보냄(반대 방향은 엔진이 좌우 반전). 색 0은 투명 |
| 얼굴 그림 | `FACEDAT.R3` (TF-DCE 압축) | **추출** → `gfx/original/facedat/<nnn>.png`, **실물 검증(눈으로)** | 높음. `TFDED.COM`을 정적으로 읽어 만든 디코더([FORMATS §6](reverse-engineering/FORMATS.md#tfdce)). 한국어판 240개 모두 입력을 정확히 소비하고 64×80(2560바이트)을 내며, 눈으로 확인한 결과 모두 알아볼 수 있는 얼굴. 얼굴은 색 0–7만 쓰고(플레인 3은 항상 0) 이 8색은 팔레트 슬롯 4를 뺀 8개 슬롯에서 같으므로([FORMATS §7](reverse-engineering/FORMATS.md#palette)) 슬롯 선택의 영향이 없음(추출기는 슬롯 0). 색 0은 불투명 |
| 공통 화면·삽화 | `PACKGRP.R3` (TF-DCE 압축) | 원작 모드 팩: 화면 틀 3개와 사건 삽화 31개 | 높음. 38개 모두 정확히 소비. 조사 노트의 "16×16 공통 타일"이 아니라 640×400 화면 틀 2개, 512×320 창 1개, 224×144 사건 삽화 31개, KOEI 로고·문구 3개, 176×112 대리석 무늬 1개. 16색을 쓰는 3개(1·2·37번)는 어느 팔레트 슬롯이 맞는지 **미확인** |
| 오프닝·엔딩 그림 | `OPGRP`, `END1GRP`, `END2GRP` | **지원 안 함** (컨테이너만 검증) | 전체 화면 한 장이 아니라 `NPK016` 압축 그림(코덱 미해독)과 크기 정보가 없는 packed planar 그림·1비트 마스크의 묶음. 크기는 `OPEN.EXE`/`END.EXE` 코드에 있음([FORMATS §9](reverse-engineering/FORMATS.md#opening)) |
| `MARK.R3`, `SSCCHR1/2.R3` | | **지원 안 함** | 배치 미해독([FORMATS §9.3](reverse-engineering/FORMATS.md#opening)) |
| 무장·아이템·마을 사람 | `BAKDATA.R3` | **추출** → `text/officers.json`·`items.json`·`townsfolk.json`, **실물 검증** | 높음. 배치는 직접 분석([FORMATS §14](reverse-engineering/FORMATS.md#bakdata)), 능력치 순서는 공개 수치로, 얼굴 번호는 눈으로 확인. 역할·플래그 바이트는 미확인 |
| 전투 맵·전투 장면 배경·캠페인 맵·도시/궁궐 화면 | `HEXZMAP`, `HEXBMAP`, `MMAP`, `SMAP`, `PMAP` (+ `MAIN.EXE` 표) | **추출** → `maps/*.json`, `gfx/original/maps/...`, **실물 검증(눈으로)** | 높음. 칩 뱅크 구성·지형 코드·이름·크기를 `MAIN.EXE`의 읽는 코드로 확인([FORMATS §10](reverse-engineering/FORMATS.md#maps)). 58개 전투 맵 모두 칩이 뱅크 안에 있고 눈으로 본 결과 강·숲·성·다리·마을이 제자리. 팔레트 슬롯은 게임이 실행 중에 고르므로 추출기는 슬롯 1(도시 0, 궁궐 2)을 씀. `SMAP`/`PMAP` 물체 목록 `(id, x, y)`의 의미는 **미확인** |
| 세이브 | `E*.R3S`·`M*.R3S` | 아직 없음(가져오지 않음) | 로드맵(7절) 참고 |

### 3.1 6바이트 테이블과 TF-DCE 압축 (요약)

`FACEDAT.R3`/`PACKGRP.R3`는 `N × [u32le 오프셋][u16le 길이]` 표 뒤에 데이터가 오는 컨테이너이며, 오프셋은 **데이터
영역 기준**이고 `N`은 파일에 없어 체인이 파일 끝과 만나는 유일한 수로 구합니다. 항목은 `TFDED.COM`(`int 62h` 상주
드라이버, "TF-DCE 5.11")의 압축 이미지로, 드라이버를 정적으로 역어셈블해 만든 디코더로 풉니다(헤더, 플레인 방식,
열 단위 뱀 순서의 명령 스트림, 마스크·사전). 전체 명세는 [FORMATS.md §5](reverse-engineering/FORMATS.md#table6)와
[§6](reverse-engineering/FORMATS.md#tfdce)에 있습니다.

## 4. 사용법

### 4.1 게임에서 원작 모드 켜기

명령줄은 필요 없습니다(OpenRCT2와 같은 방식, [DECISIONS.md](DECISIONS.md) D10).

1. 게임(네이티브 빌드)을 실행하고 타이틀에서 **원작 데이터**를 고릅니다.
2. **원작 폴더 고르기…** → 게임 안의 폴더 탐색기에서 원작 파일이 든 폴더(`DISK1.R3I`, `MAIN.EXE`, `HEXZMAP.R3`
   등이 바로 들어 있는 폴더, DOSBox 패키지라면 그 안의 `GAME` 같은 폴더)로 들어갑니다. 원작 파일이 있는 폴더는
   ★로 표시되고, 폴더에 들어가면 `hero-tools original probe`와 같은 판정과 근거가 보입니다. 들어간 폴더가 원작
   폴더가 아니고 바로 아래 한 폴더에만 원작 파일이 있으면(DOSBox 패키지의 `res/hero` → `GAME`) **"GAME 폴더
   사용"**이 함께 나옵니다.
   - 조작: 확인(Enter·Z·클릭)으로 폴더에 들어가고, Backspace·`..`로 상위 폴더, 글자 키로 그 글자로 시작하는 폴더로
     이동(Z·X 제외), Page Up/Down, 휠. Windows에서는 드라이브 최상위에서 한 번 더 올라가면 드라이브 목록입니다.
   - 경로를 알면 **경로 입력…**이나 **Ctrl+V**(macOS는 Cmd+V)로 경로 줄을 열어 입력·붙여넣기하고 Enter로 그
     폴더로 갑니다(Esc·우클릭: 목록으로, Ctrl+Backspace: 지우기). 탐색기의 "경로로 복사"처럼 따옴표가 붙은 경로와
     원작 파일(`MAIN.EXE` 등)의 경로도 받고, 상대 경로는 보고 있는 폴더 기준, `D:`는 그 드라이브의 최상위입니다.
3. 지원하는 판본(한국어 DOS/V, 중국어 DOS)이면 **이 폴더 사용**이 켜집니다. 고르면 설정에 경로와 "원작 모드 켬"을
   저장하고 데이터를 다시 불러옵니다.
4. 로딩 화면이 기본 팩을 읽은 뒤 원작을 **메모리에서** 원작 모드 팩(4.5절과 같은 파일)으로 변환해 기본 팩 위에
   얹고 플레이합니다. 디스크에는 아무것도 쓰지 않고, 원작 폴더는 읽기만 합니다. 변환 시간은 한국어 DOS/V 실물에서
   release 빌드 약 0.1초입니다.
5. 다음 실행부터는 바로 원작 모드로 시작합니다. 타이틀 → 원작 데이터에서 **기본 팩으로 플레이** / **원작 모드로
   플레이**로 오갈 수 있고(경로는 기억), 하단의 팩 이름(`영걸전 원작 모드`)으로 지금 모드를 알 수 있습니다.

* **폴더가 사라지거나 바뀌면**: 실행 때 "원작 폴더를 찾을 수 없습니다" / "원작 폴더를 쓸 수 없습니다"(지원하지 않는
  판본, 근거 표시) / "원작 변환 실패"(손상 파일) 화면이 원인을 보여 주고 **다시 시도 · 다른 폴더 고르기 · 기본 팩으로
  계속 · 종료**를 제공합니다. "기본 팩으로 계속"은 원작 모드를 끄고(경로는 남김) 기본 팩으로 플레이합니다. 변환된 팩이
  로드·검증에 실패해도 같은 화면입니다. 일부 종류만 변환하지 못하면 알림을 띄우고 그 부분은 기본 팩 그림을 씁니다.
* **세이브**: 원작 모드의 세이브는 팩 id `original`로 기본 팩 세이브와 분리됩니다(파일로 만든 팩과 같은 id).
* **우선순위**: `--data` 또는 `EIKETSUDEN_DATA`로 팩을 지정해 실행하면 그 팩을 그대로 플레이하고 원작 모드 설정은
  적용하지 않습니다(타이틀에 "원작 데이터"도 나오지 않음).
* **웹 빌드에는 없습니다.** 브라우저에는 로컬 폴더를 읽는 경로가 없습니다(향후 File System Access API/OPFS로 검토).
* 설정은 사용자 데이터 폴더의 `settings.json`(`original_dir`, `original_mode`)에 있습니다.

### 4.2 판본 확인 (프로브)

아래 4.2–4.5절의 명령줄 도구는 개발·검증·매니페스트 공유용입니다.

```sh
cargo build --release -p hero-tools      # target/release/hero-tools(.exe)
hero-tools original probe "D:/Games/영걸전/GAME" --out manifest.json
```

* 식별된 판본·신뢰도·근거, 파일 수, LS11 아카이브 검증 결과, 지원 범위를 출력합니다.
* `--out`을 주면 공유 가능한 매니페스트를 씁니다(설치 폴더 안에는 쓰지 않습니다).
* 폴더를 읽을 수 없으면 종료 코드 1, 명령줄 오류는 2입니다. 판본을 모르는 폴더도 프로브 자체는 성공(0)입니다.

### 4.3 추출

```sh
hero-tools original extract "D:/Games/영걸전/GAME" --out "D:/영걸전-원작" [--text] [--sprites] [--portraits] [--maps]
```

* 종류 옵션이 없으면 모든 종류(텍스트, `--text`가 함께 쓰는 이름, 스프라이트, 얼굴, 맵)를 시도합니다.
  종류를 고르지 않았을 때는 판본이 지원하지 않는 종류가 있어도 보고만 합니다.
* 종류를 **명시했는데** 추출할 수 없으면 실패로 보고하고 종료 코드 1을 돌려줍니다.
  어느 종류든 오류가 나면(`partial`/`failed`) 역시 1입니다.
* `--edition korean-dos|chinese-dos`는 식별을 건너뜁니다(식별 결과는 근거로 함께 기록됩니다).
* 출력 폴더는 새 폴더·빈 폴더·이전 추출 결과(`index.json`) 중 하나여야 합니다. 이전 결과면
  `index.json`에 적힌 파일만 지우고 다시 씁니다. 다른 파일이 든 폴더는 거부합니다. 쓰는 동안에는 쓴 파일을
  `.hero-import-partial`에 적어 두므로, 도중에 멈춘 실행이 남긴 폴더는 다음 실행이 그 파일들만 지우고 처음부터 다시 씁니다.

출력 구조 (미디어 오버레이):

```text
<out>/index.json                         판본, 종류별 상태(extracted/partial/failed/unsupported/missing-source),
                                         요약·주의사항·오류, 원본 파일 SHA-256, 쓴 파일 목록
<out>/gfx/original/palettes.json         MAIN.EXE 팔레트 9슬롯 + 슬롯별 관찰 메모
<out>/gfx/original/sprites.json          아카이브별 기본 팔레트 슬롯, 항목마다 배치·크기·미디어 키,
                                         항목 묶음(병종·효과; FORMATS §8)
<out>/gfx/original/hexbchr/000.png ...   미디어 키 original/hexbchr/000 (인덱스 PNG, 색 0 투명)
<out>/gfx/original/sheets/hexbchr.png    아카이브 전체를 한 장에 모은 확인용 시트(16개씩 한 줄,
                                         다른 슬롯을 쓰는 항목은 sheets/<파일>-slot<N>.png)
<out>/gfx/original/facedat/000.png ...   얼굴 240개(64×80, 색 0 불투명), 미디어 키 original/facedat/000
<out>/text/snr0.json ...                 장(장면 → 대화·문자열, 블록 → 트리거 레코드 → 명령; 10절)
<out>/text/snr0.txt ...                  같은 내용의 읽기용 목록
<out>/text/townsfolk_talk.json           장·마을별 마을 사람과 대사(IPPAN0), ippan0m.json = 문자열 모음
<out>/text/officers.json, items.json,    BAKDATA.R3 마스터 표
          townsfolk.json
<out>/maps/battle.json                   전투 맵 목록(이름·크기·칩 세트), 지형 표(코드·이름·전투 장면
                                         배경), 칩별 지형 통계(FORMATS §10)
<out>/maps/battle/000.json ...           전투 맵 하나의 칩 격자(16 px)와 지형 격자(32 px 칸)
<out>/maps/scene.json, campaign.json,    전투 장면 띠, 캠페인 맵(타일·행군로), 도시·궁궐 화면(타일·
<out>/maps/town.json                     보행 격자·표시 지점·물체)
<out>/gfx/original/maps/battle/000.png   전투 맵 그림, battle/chips-1.png·chips-2.png = 칩 뱅크(번호순 16개씩)
<out>/gfx/original/maps/{scene,campaign,town}/...  전투 장면 띠, 캠페인 맵, smap-/pmap- 화면
```

### 4.4 미디어 오버레이를 게임에서 쓰기

```sh
eiketsuden --original "D:/영걸전-원작"          # 또는 EIKETSUDEN_ORIGINAL=D:/영걸전-원작
```

* 게임의 미디어 저장소(텍스처·사운드·아이콘 목록)가 **오버레이 폴더를 먼저, 그다음 데이터 팩을** 찾습니다.
  오버레이에 없는 파일은 팩에서 읽습니다. 규칙·대사 같은 팩의 텍스트 파일은 오버레이하지 않습니다(미디어로 찾는
  색인 파일과 `credits.txt`는 예외, 아래).
* `index.json`이 없는 폴더(예: 설치 폴더 자체)를 지정하면 경고를 남기고 무시합니다.
* **현재 한계**: 추출물의 키(`original/...`)는 기본 팩이 쓰는 키(`portraits/liu_bei`, `units/archer_player` 등)와
  다르므로, 오버레이를 켜도 기본 게임 화면이 자동으로 원작 그림으로 바뀌지는 않습니다. 팩(모드)이 `original/...`
  키를 참조하거나, 오버레이 폴더 안에 팩과 같은 키 이름으로 파일을 두면(예: `gfx/portraits/liu_bei.png`) 그 파일이
  우선합니다. 원작 그림을 기본 팩의 키로 옮겨 게임 화면에 쓰는 것은 오버레이가 아니라 원작 모드 팩(4.5절)이
  합니다. 이 오버레이는 추출물을 살펴보거나 모드가 `original/...` 키를 직접 참조할 때 쓰는 경로입니다.
* **오버레이가 바꾸는 것**: 이미지·사운드와 색인 파일 4종(`gfx/ui/icons.toml`, `gfx/units/units.toml`,
  `gfx/tiles/terrain.toml`, `gfx/fx/fx.toml`)이고, 오버레이의 `credits.txt`는 팩 것들 앞에 덧붙습니다. 모두 파일마다
  따로 찾으므로(오버레이 → 팩), 프레임·타일 크기가 다른 시트(원작의 48×48/64×64 유닛 스프라이트, 다른 칩 크기의 타일 아틀라스)를 오버레이에
  넣을 때는 그 크기를 적은 색인도 함께 넣어야 합니다. 오버레이 색인은 팩 색인을 **통째로** 대체하므로 팩이 쓰는
  모든 키를 적어야 합니다(없으면 그 키는 기본 16×16 프레임이나 평면 색으로 그려집니다). `hero-tools original
  extract`는 색인 파일을 쓰지 않으므로 추출물 그대로는 팩 색인을 씁니다. 원작 그림을 게임 화면에 쓰는 일반적인
  방법은 오버레이가 아니라 원작 모드 팩(4.5절)입니다.
* **웹 빌드는 지원하지 않습니다.** 브라우저에는 로컬 폴더를 읽는 경로가 없어 `--original`이 없습니다
  (향후 File System Access API/OPFS로 검토).

### 4.5 원작 모드 팩을 파일로 만들기 (개발·검증용)

게임은 4.1절처럼 같은 변환을 실행할 때마다 메모리에서 합니다(`hero_import::pack::build_pack`). 이 명령은 같은
팩을 파일로 써서 내용을 살펴보거나 `hero-tools validate`·`info`로 검사할 때 씁니다.

```sh
hero-tools original pack "D:/Games/영걸전/GAME" --out data/original [--base data/base] [--edition korean-dos]
eiketsuden --data data/original
```

* 기본 팩을 확장하는 **레이어드 팩**(8절)을 씁니다: `pack.toml`(`id = "original"`, `extends` = 기본 팩까지의 상대 경로,
  `canvas = [640, 400]`, 원작 전투 화면 틀 `[presentation.battle_frame]`), 변환 기록 `original-pack.json`, 그리고 변환된 미디어만. `--base`를 생략하면 `--out` 옆의
  `base` 폴더를 기본 팩으로 씁니다. 두 폴더는 같은 드라이브에 있어야 합니다(`extends`는 상대 경로만 허용).
* 쓴 팩은 기본 팩의 규칙 파일을 원작 값으로 바꾼 복사본을 가지므로(8절), 기본 팩이 바뀐 뒤에는 기본 팩의 새 병종·필드·책략을
  가립니다. 그래서 `original-pack.json`에 쓸 때의 기본 팩 체인 지문(`base_fingerprint`: 모든 `pack.toml`과 읽힌 글 파일의
  SHA-256)을 적어 두고, `hero-tools validate`와 게임(연 데이터 팩 체인: `--data`·`EIKETSUDEN_DATA`·설정의 데이터 폴더, 타이틀 전 알림과 로그)이 지금의 기본 팩과 다르거나
  변환기의 팩 형식 버전이 다르면 다시 변환하라고 경고합니다(그 팩을 확장한 모드도). 지문은 기본 팩 체인의 글 파일 전체라
  원작 팩과 상관없는 수정(대사 오탈자 등)에도 경고가 납니다(놓치는 것보다 낫다고 봄). 실행마다 새로 변환하는 게임 안 원작 모드에는 해당하지 않습니다.
* 쓴 뒤에 `hero-tools validate`와 같은 검사를 돌려 결과를 보여 주고, 변환에 실패한 종류가 있거나 팩에 오류가 있으면
  종료 코드 1을 돌려줍니다. 출력 폴더는 새 폴더·빈 폴더·이전에 이 명령이 쓴 팩(`original-pack.json`)만 허용합니다(도중에 멈춘 실행은
  추출과 같이 `.hero-import-partial`로 정리).
* `MAIN.EXE`의 팔레트를 찾지 못하면 그림을 쓰지 않습니다(오버레이와 달리 회색 대체 그림으로 플레이하게 두지 않음).
* 들어가는 것(한국어 DOS/V 실물 기준, 기본 팩 0.2.0 위):

  | 종류 | 원본 | 결과 | 매핑 규칙 |
  |---|---|---|---|
  | 얼굴 | `BAKDATA` + `FACEDAT` | `gfx/portraits/<무장>.png` 108/118명 | 기본 팩 무장과 `BAKDATA` 무장을 **이름**으로 대응(중문판은 한자 이름). 표기가 다른 5명(장료=장요, 기령=기영, 송헌=송겸, 왕해=왕개, 관흥=관훙)은 별칭 표, 이름이 겹치는 우금(于禁/牛金)은 일본어 읽기(`ｳｷﾝ`)로 구분. 기본 팩이 새로 만든 인물 등 대응이 없는 10명은 기본 팩 얼굴 그대로이며 `original-pack.json`에 사유와 함께 기록 |
  | 무장 | `BAKDATA` + `SNR0D`–`SNR4D`(합류) + `FACEDAT` | `officers.toml`(`officers`), 추가 무장의 `gfx/portraits/orig_p<번호>.png` | 체인(기본 팩)의 무장을 모두 옮기고, 얼굴과 같은 이름 대응으로 원작 무장이 하나로 정해지면 **통솔·무력·지력**(0–100으로 자름, FORMATS §14 [검증])과 **병종·레벨·장비**를 원작 값으로 바꾼다(팩에 없는 병종은 체인 것을 둠; id·이름·얼굴·소개·군주 여부는 체인 것). `BAKDATA`는 시나리오가 바꾸기 전의 상태이고, 합류 때의 병종 변경·레벨 증가(`set_class`·`add_levels`)는 원작 캠페인이 `@class`·`@level`로 재생한다(FORMATS §14, D21). 이름이 같은 원작 무장이 여럿이면 그 기록들이 모두 같은 값만 옮긴다(능력치가 다르면 능력치를, 병종·레벨·소지품이 다르면 병종·레벨·장비를 체인 것으로 두고 적는다). 원작 캠페인(서장~4장)에서 유비군에 합류하는(`set_country` 소속 0, `set_allegiance` 군 0) 원작 무장 중 체인 무장이 맡지 않는 사람은 `orig_p<BAKDATA 번호>`로 **더한다**: 이름·병종(원작 병종의 스프라이트를 쓰는 팩 병종)·레벨(1–레벨 상한)·능력치와 소지품 중 무기·병법서·말(팩 아이템 종류가 무기·방어구·장신구, 칸마다 첫 것)과 얼굴까지 `BAKDATA` 값. 전에는 이들의 합류가 빠졌다. 바꾼 값과 더한 무장은 `original-pack.json`의 `officers`에 적는다 (DECISIONS D21) |
  | 유닛 | `HEXZCHR` | `gfx/units/<병종>_<진영>.png` 19병종 × 3, 무장 전용 5종 × 3, 혼란 1종 × 3 + `units.toml`(32×32 프레임, `[officers]`, `[statuses]`) | 병종 순서대로 두 색 아이콘(32×32 두 프레임, 오른쪽을 봄). 엔진 시트의 오른쪽·아래 열은 원본, 왼쪽·위 열은 좌우 반전, 걷기 행은 두 프레임 교대(대기 애니메이션이 원작처럼 두 프레임을 오감), 공격 = 첫 프레임, 피격 = 둘째 프레임. **주황 = 아군·우군, 초록 = 적군**은 원작 코드가 고르는 대로입니다([FORMATS §8.2](reverse-engineering/FORMATS.md#planar)). 원작처럼 유비는 처음 세 병종에서 38–40(깃발 단병·장병·백마 전차), 여포는 45, 조조는 46으로 그립니다(진영과 상관없이 한 그림). 유비가 그 밖의 병종이면 병종 그림입니다(원작에는 이 경우가 없고, 그 코드는 38 + 병종 번호를 그대로 씀). 혼란한 유닛은 원작처럼 누구든 43(아군 측)·44(적군 측)로 그립니다(상태 `& 0x02` = 혼란, 무장 아이콘보다 먼저). 무장 아이콘 하나를 만들지 못하면 그 무장만 병종 그림으로 두고 `original-pack.json`에 적습니다 |
  | 지형 타일 | `HEXZMAP` + `HEXZCHP` | `gfx/tiles/terrain.png`·`terrain.toml`(`tile_size = 32`) | 원작 맵의 2×2 칩 칸(32 px, 유닛이 움직이는 격자) = 엔진 타일 하나. 지형마다, 이웃 마스크(`auto` 층의 4비트)마다 **원작 맵 58개에서 가장 자주 나오는 칸**을 고름. 맵에 없는 마스크는 가장 가까운 관찰 마스크를 빌리고, 이웃과 무관한 지형(평지·마을·병영 등)은 가장 흔한 칸 하나. 원작에 없는 `road`는 평지 칸 |
  | 전투 | `SNR0D`·`SNR1D` + `BAKDATA` | `battles/<전투>.toml`(기본 팩에 남은 시험 전투 `p1_sishui`를 대체; 기본 팩이 서장·1장 21개를 담던 때는 21개) | 기본 팩 전투마다 원작의 같은 전투(`battles.rs`의 대응표)를 찾아 **원작 맵(`use = "hexz_NN"`), 턴 제한, 배치 칸, 적·우군 명단**(무장 = 얼굴과 같은 대응(별칭 포함)의 기본 팩 무장, 아니면 `BAKDATA` 이름과 그 인물의 무력·지력·통솔을 가진 일반 유닛; 병종·레벨·AI는 원작 값), **보물**(금·아이템 칸), 유비의 **목표 칸**, 나중에 합류하는 부대와 그 **합류 조건**(턴·칸·영역·격파·인접 → `spawn` 이벤트)을 원작대로 둔다. 이름·목표 문구·전후 대사 장면·음악·보상과, 남은 무장만 가리키는 기본 팩 이벤트(관우-화웅 일기토 등)는 기본 팩 것. 기본 팩이 원작 인물의 자리를 다른 인물로 바꾼 곳(산적 두목 창희·하곤·석맹)은 역할 대응표로 그 무장이 맡는다. 옮기지 못한 것(원작 명단에 없는 기본 팩 무장, 그 무장이나 기본 맵의 칸을 가리키는 이벤트·조건, 기본 팩의 증원 그룹)은 전투 파일 주석과 `original-pack.json`에 적는다 |
  | 지형 규칙 | `MAIN.EXE`(병종 → 이동 종류, 이동 종류 × 지형 비용, 지형 효과 표) | `rules/terrain.toml`(`pack.toml`의 `[rules] terrain`) | 팩 체인의 지형 규칙에서 원작 지형 코드가 있는 지형의 이동 비용(255 = 들어갈 수 없음)과 방어(지형 효과)를 원작 값으로 바꾼다. 원작의 이동 종류 넷은 같은 병종들에게 팩이 준 이동 종류 이름을 따르고, 팩이 원작이 묶은 병종들을 다른 이동 종류로 나누면 변환하지 않는다. 원작의 성문(코드 10)은 닫힌 성문이라, 기본 팩의 열린 `gate`는 그대로 두고 그 복사본인 `closed_gate`(성문 타일, 체인이 쓰지 않는 글리프, 예: `K`)를 더해 전투 맵의 규칙 격자에 쓴다(규칙 변환이 실패하면 성문이 있는 맵은 변환하지 않아 기본 전투가 남는다). 원작 이동 종류 둘이 비용이 다른데 팩에서 같은 이름이면 변환하지 않는다. 기본 팩과 다른 값은 `original-pack.json`에 적는다. 원작에 없는 지형(길)과 그 밖의 필드는 팩 것 |
  | 병종 규칙 | `MAIN.EXE`(공격·방어 계수, 이동력, 공격 범위, 병력·병력 성장 표) | `rules/classes.toml`(`pack.toml`의 `[rules] classes`) | 팩 체인의 병종 중 원작 병종의 스프라이트로 그리는 병종(여럿이면 그중 스프라이트와 같은 id인 병종, 그런 병종이 없으면 그 스프라이트의 병종 모두)의 `atk`·`def`(계수 ÷ 5), `move`, `hp`(표 × 100)·`hp_growth`(표 × 10), `range`(0 `adjacent4`, 1 `adjacent8`, 2 `archer`, 3 `crossbow`, 4 `catapult`, 255 없음)를 원작 값으로 바꾼다. 기본 팩과 다른 값은 `original-pack.json`에 적는다. 습득 목록은 책략 규칙 행, 그 밖의 필드는 팩 것 |
  | 책략 규칙 | `MAIN.EXE`(책략 도달·MP 표, 책략 × 병종 습득 레벨 행렬) | `rules/strategies.toml`(`[rules] strategies`), `rules/classes.toml`의 `strategies` | 원작 책략 36개를 순서대로 기본 팩 id(`STRATEGY_IDS`)로 옮겨 MP와 도달(0 `range8`, 1 `range12`, 2 `range20`, 3 `range28`)을 원작 값으로 바꾸고, 공격 위력·사기 저하량·지원의 병력·사기 기본값(600/1200/1800, 30/40/50)과 大 지원의 도달(단계별 range8/12/20)을 원작 식대로 넣는다(FORMATS §10.4 책략 효과). 병종 규칙의 대상 병종은 습득 목록을 원작 행렬대로 다시 쓴다(체인에 있는 책략만). 승급한 병종은 엔진 규칙대로 앞 병종들의 목록도 알고, 그래서 원작 목록에 없거나 더 이른 레벨의 책략이 생기면 적는다(한국어판에는 없음). 설명과 원작에 없는 책략(기본 팩의 폭파)은 팩 것 |
  | 아이템 규칙 | `BAKDATA`(아이템 이름) + `MAIN.EXE`(회복 식) | `rules/items.toml`(`[rules] items`) | 팩 체인의 아이템을 복사하고, 원작 회복 아이템(27–29 사기 30/40/50, 30–32 병력 600/1200/1800, 52–54 둘 다; 이름으로 기본 팩 아이템과 맞춤)의 `effects`와 설명의 괄호 속 수치를 원작 값으로 바꾼다(FORMATS §10.4). 원작 식에서는 사용할 때 10 % 안의 난수가 더해진다(RULES §10; 게임 규칙을 쓰지 못해 원작 식이 아니면 난수 없이 이 값). 설명의 수치는 종류별로 한 번에, 한 곳에만 있을 때만 바꾸고 아니면 변환 기록에 남긴다 |
  | 게임 규칙 | (책략 규칙을 옮길 때) | `rules/game.toml`(`[rules] game`) | 팩 체인의 게임 규칙에 `strategy_formulas = "original"`만 바꿔 쓴다: 지원 보너스·난수, 혼란 명중과 풀림, 사기 저하 혼란, 최소 피해가 원작 식(RULES §5, FORMATS §10.4 책략 효과) |
  | 전투 화면 틀 | `PACKGRP` 1번 + `MAIN.EXE`(팔레트) | `gfx/ui/orig_battle_frame.png`(640×400), `pack.toml`의 `[presentation.battle_frame]` | 원작 전투 화면 틀을 전투 맵과 같은 팔레트 슬롯으로 그린다(틀은 맵과 함께 화면에 있어 색 8–15가 맵의 팔레트를 따른다, [추론]). 맵 구멍(416×352, 32px 칸 13×11)·오른쪽 파란 패널·위 제목 칸·오른쪽 위 검은 칸의 좌표는 틀에서 잰 값이고, 변환할 때 맵 구멍이 비어 있는지(색 0) 확인해 다르면 틀을 쓰지 않는다. 엔진은 맵을 구멍에, 유닛·지형·예측 창을 패널에, 전투 이름·턴·국면을 제목 칸에, 날씨·금을 검은 칸에, 날씨 아이콘을 오른쪽 그림 칸에 그리고, 기능·아군·적군 버튼을 누르면 전투 메뉴·아군 부대 일람·적군 부대 일람을 연다(DECISIONS D15) |
  | 사건 삽화 | `PACKGRP` 3–33번 + `MAIN.EXE`(팔레트) | `gfx/pictures/orig_<번호>.png` 31개(224×144) | 색 0–7만 써서 슬롯 4 말고는 같다(얼굴과 같음). 장 이야기의 `show_picture <번호>`는 드라마 `@picture orig_<번호>`가 되고, 원작처럼 그 뒤의 서술 동안만 보인다(서술이 아닌 다음 명령에서 `@picture none`). 두 번째 피연산자(`variant`, 블록 첫머리 0·중간 1)의 뜻은 모르며 쓰지 않는다. 엔진은 그림을 테두리를 둘러 텍스트 상자 위 가운데에 정수 배로 그린다(더 크면 줄임). 전투 이벤트 스크립트와 전투 준비 레코드의 삽화(맥성 23·24번, 28·29번)는 아직 옮기지 않는다(BACKLOG). 서장·1장은 기본 팩 이야기라 쓰지 않는다 |
  | 캠프 화면 틀 | `PACKGRP` 0번 + `MAIN.EXE`(팔레트) | `gfx/ui/orig_camp_frame.png`(640×400), `[presentation.camp_frame]` | 원작 메인 화면 틀(색 0–7만 쓰므로 슬롯 4 말고는 같다). 보기 구멍(511×322, 왼쪽 장식 한 픽셀을 피함)·초상 칸(64×80)·돈·레벨·현재영토 칸·아래 두 칸의 좌표는 틀에서 잰 값이고, 변환할 때 보기 구멍이 비어 있는지 확인한다. 엔진은 캠프 화면들을 보기 구멍을 캔버스 삼아 그리고, 군주(첫 무장) 얼굴·금·레벨·장소·캠프 제목·플레이 시간을 칸에 채운다(DECISIONS D15). 아래 오른쪽 네 아이콘은 아직 그림일 뿐이다 |
  | 상태 창 | `PACKGRP` 2번 + `MAIN.EXE`(팔레트) | `gfx/ui/orig_status.png`(512×320), `[presentation.status_frame]` | 원작 부대 정보 창을 팔레트 슬롯 0으로 그린다(16색 그림이고 어느 슬롯인지는 [추론]: 메인 화면에서 열리는 창). 제목·무장 칸 6개(유닛 아이콘·Lv·병력)·초상·이름·부대Lv·통솔력·무력·지력·아래 칸(병과)·큰 칸(장비·책략)·페이지·화살표·나머지·종료의 좌표는 그림에서 잰 값이고, 변환할 때 초상 칸과 무장 칸 6개가 검은지(색 0) 확인한다. 엔진의 캠프 무장 정보 화면이 이 창에 그려진다(DECISIONS D15) |
  | 음악 | `MUSIC.R3`·`OPMUSIC.R3`·`EDMUSIC.R3` | `bgm/<키>.wav` 10개(22050 Hz 16비트 모노, 약 43 MB) | 재생기를 옮겨 곡을 연주하고 자체 OPL2 에뮬레이터로 렌더링한다. 인트로와 루프 한 바퀴를 담고 WAV `smpl` 청크에 루프 시작을 적어, 게임이 인트로를 한 번 튼 뒤 루프를 반복한다(트랙마다 루프 지점이 다른 곡은 루프 길이의 최소공배수가 600초 안일 때만 이음새 없이, 아니면 처음부터 한 번; FORMATS §16). 기본 팩의 음악 키를 대신한다: `title` = OPMUSIC 0, `ending` = EDMUSIC 0, `camp` = 12(출진 준비), `peace` = 5(회의장), `tension` = 11(적 진영의 장면), `sad` = 2(황제의 탄식·방통의 죽음), `defeat` = 4(유비의 죽음), `boss` = 16(여포 같은 강적 등장), `enemy` = 9(전투 중 적장의 대사), `battle` = 18(어느 시나리오도 부르지 않아 게임이 전투 중 트는 곡으로 봄). 곡 배정은 시나리오 `0x38`의 문맥으로 고른 [추론]이다. `hero-tools original pack`은 팩에 쓰고(release 빌드로 팩 변환 전체 약 30초), 게임은 메모리 변환을 마치고 시작한 뒤 백그라운드에서 렌더링해 곡이 끝나는 대로 붙인다(그 전까지는 기본 팩 음악, DECISIONS D16) |
  | 전투 중 이벤트 | `SNR0D`·`SNR1D` 트리거 레코드 + `SNR0M`·`SNR1M` + `MAIN.EXE`(칸 변경 표) | 전투 파일의 `[[events]]`, `dramas/original_battles.drama`(대사 장면 45개), `gfx/maps/hexz_NN_X_Y_OP.png`(바뀐 칸 7개) | 전투 블록의 그룹 3부터를 **단계**로 보고(FORMATS §13.2), 레코드마다 트리거(턴·인접·칸·영역·격파)와 동작을 옮긴다: 대사·안내문·일기토 → 대사 장면(화자 = 같은 이름의 기본 팩 무장, 아니면 원작 이름), 합류 → `spawn`, AI 변경 → `set_ai`, 레벨 → `level_up`, 퇴장 → `retreat`, 금·아이템, 성문·적교 → `set_terrain`(바뀐 칩으로 그린 칸 그림), `battle_end` → 승리, 단계 넘김 → `set_stage`. 경로 플래그(계교)와 한 번만 실행하는 플래그는 변환할 때 판정하고, 한 레코드가 켜고 다른 레코드가 검사하는 플래그(하비의 세 장수, 계교의 군량고)는 전투 플래그 `orig_<전투>_<번호>`와 `when` 조건이 되고, 그 플래그가 성립하지 않을 때 스크립트가 하는 일은 `unless` 조건의 이벤트가 된다. 원작 목표 문구 변경은 `set_objective`가 된다. 기본 팩이 같은 계기의 이벤트를 유지하면(대부분의 일기토 등) 그 이벤트가 이야기를 맡고 원작의 나머지 동작(퇴장·단계 넘김)을 더한다(DECISIONS D12) |
  | 원작 캠페인 | `SNR0D`–`SNR4D`·`SNR0M`–`SNR4M` | `battles/c<장>_s<장면>_b<블록>.toml`(서장·1장 19개(실물 변환 확인, 플레이 확인 전), 2장 11, 3장 21, 4장 13), `dramas/original_chapters.drama`(이야기 장면), `campaign.toml` | 서장(반동탁 연합)부터 원작 2–4장(관도~장판파, 형남~이릉, IF)까지 원작의 이야기와 전투로 캠페인 전체를 만들어 원작의 엔딩까지 잇는다(DECISIONS D18·D21). 캠페인은 체인 캠페인의 제목·시작 무장(유비·관우·장비)·시작 금만 쓰고 노드는 모두 원작에서 만든다. 전투는 원작 헤더로 만든 합성 기본 전투를 원작대로 다시 짜고(이름 = 맵 이름, 목표 = 원작 목표 문구, 승리 = 적장 격파 또는 전멸과 원작 목표 칸, 보병이 못 들어가는 배치 칸은 뺌), 마을·캠페인 맵 블록은 대사 장면이 된다(잡담은 빠짐, 선택지·예/아니오 질문 → `@choice`(그룹을 넘기지 않는 선택은 다시 물음), 다른 블록으로 가는 선택 → 루트 플래그와 캠페인 분기, 게임 오버 선택 → 게임 오버 엔딩, 합류 → `@join`, 다른 군으로 옮겨짐 → `@away`(진행을 지킨 채 출진만 못 함, 다시 소속 0이 되면 `@join`으로 복귀), 아이템·음악 → 드라마 명령, 상점 = `set_shop_items`가 바꿀 때까지 이어지는 목록(첫 목록은 서장이 첫 전투 전에 정함)). 여러 무장이 저마다 출진을 묻는 곳은 "누구의 뜻을 따를까" 선택지가 되고(신야 농성·박망파), 다른 블록에서 이어지는 전투(강릉)는 한 전투로 읽는다. 전투의 마지막 승리 스크립트와 그 뒤 에필로그는 전투의 `outro` 장면이 된다(고성 전투 뒤 장비의 복귀, 여남 뒤 유벽의 합류 선택 등. 그 금은 전투 보상, 전투 중 우군으로 나오는 무장은 출진 금지). 출진 설정에 이름이 든 무장은 그 전투 때 군에 있으면 필수 출진, 없으면(아직 합류 전이거나 `@away`로 떠나 있으면) 그 칸에 서는 AI 우군이 된다. 소속은 시작 군(유비·관우·장비)에서 출발해, 이야기가 그 전투에 이르는 **경로**의 `@join`·`@away`로 판단한다(다른 루트의 장면과 승리·패배 뒤의 장면은 그 갈래로만 셈, 이슈 #81). 경로에 따라 소속이 갈리는 무장은 그 칸에 플레이어 쪽으로 서고(군에 있고 떠나 있지 않으면 군의 무장, 아니면 `officers.toml`의 무장), 지금 원작 데이터에는 그런 무장이 없다. 장면 안 선택지의 갈래와 전투 이벤트의 합류는 아직 경로로 나누지 않는다(BACKLOG). 이야기가 한 번도 옮기지 않는 무장은 시작 무장이거나 어디선가 합류할 때만 군에 있고, 합류하지 않는 동맹 무장(사수관의 공손찬·도겸, 1장의 조조군 장수 등)은 그 칸의 우군이다(#84). 원작이 출진 설정·명단을 루트의 플래그(`if_flags`, 손님 칸의 `requires_flag`)로 고르는 전투는 다른 전투가 되는 플래그 조합마다 `<전투>_f<플래그>…` 변형이 생기고, 캠페인이 캠프 앞 `<전투>_which`에서 플래그로 고른다(계교 133, 진창·장안 38, `c4_s2_b2` 218; DECISIONS D23). 출진 설정이 "나중에 합류"로 남겨 둔 칸의 무장은 `join_battle` 이벤트 때 그 칸에 도착한다(원작 MAIN.EXE도 그 칸에 놓음, FORMATS §13.4): 그때 군에 있으면 플레이어 쪽(군의 무장), 아니면 우군. "아무 유닛이 칸에 서면" 레코드는 금·아이템을 줄 때만 보물이고 나머지는 `reach` 이벤트다(#86). 전투 중 `set_allegiance`는 `set_country`처럼 군 0이면 전투 뒤 합류, 다른 군이면 전투 뒤 `@away`가 된다. 이야기 속 레벨·병종 변경은 `@level`·`@class`가 된다(군을 떠났다 돌아오는 무장은 돌아온 뒤에, 아직 합류하지 않은 무장의 것은 엔진이 합류할 때까지 맡아 둔다: 4장 오의 장수들의 +11, DECISIONS D24). 전투가 `battle_end`로 다른 전투 맵에 이어지면(장판파: 백성이 첫 맵을 건너면 둘째 맵, 와구관) 두 전투로 나뉜다: 첫째는 `c<장>_s<장면>_b<블록>`, 둘째는 그 뒤에 `_2`가 붙고(원작이 그 레코드에서 세우는 출진 설정·명단·맵, 자기 출진 준비 캠프를 가짐), 첫째의 승리 스크립트가 첫째의 `outro`다. 출진 설정에 이름이 든 민중(장판파의 백성 셋)은 출진 칸이 아니라 그 칸에 서는 아군 유닛(`civilian`)이 되고, 개막이 마을 칸으로 행군시키며(`march`) 그 칸에 닿는 것이 그 맵의 승리다. 적은 그들을 이름으로 노린다. 전투의 개막(그룹 2의 `run` 레코드: 양쪽의 첫 대사, 일기토 도전과 패자의 퇴각, 켜는 플래그)과 그룹 3 이후의 단계 가운데 첫 감시 단계 앞에 `run` 레코드만 있는 단계(허창 2 `c4_s1_b7`·`c3_s2_b10`의 개막)는 전투 시작 이벤트(1턴 아군 페이즈)가 된다. 개막이 하는 일 가운데 유닛에 이미 반영된 것(AI·참전·목표 문구)은 되풀이하지 않고, 루트 플래그로 갈리는 줄은 같은 시점의 조건부 이벤트다(진창·장안의 방통, 플래그 38). 출진 설정(그룹 0·1)이 출진 전에 보여 주는 삽화·서술·제목·대사(맥성의 삽화 23·24와 장 제목, 허창 2의 회의)는 전투 앞의 `<전투>_before` 장면에 들어가고, 출진 안내 대사("부대를 편성해 주십시오")와 예/아니오 질문은 뺀다. 전투를 끝내는 이벤트는 마지막 단계의 것뿐 아니라 앞 단계의 `battle_end`·`goto_block`도 끝냄 플래그를 켜서 outro의 승리 스크립트를 건너뛰게 한다(토채·성채 점령). 화계·수계 같은 한 편의 사기·병력 절반(`2D`)은 동작 `halve`, 전투 이벤트의 `game_over`(4장 업성 붕괴)는 패배가 된다. 출진 설정이 `battle_end`로 맵을 불러오는 전투(4장 마지막 둘, 이슈 #95)도 전투로 읽는다 |
  | 전투 맵 | `HEXZMAP` + `HEXZCHP` + `MAIN.EXE`(칩 뱅크 목록) | `maps/original.toml`의 `[[map]]` 58개(id `hexz_00`–`hexz_57`) + 맵마다 그림 층 `gfx/maps/hexz_NN.png` | **그림 층** = 맵의 칩 격자를 게임과 같은 뱅크(FORMATS §10.2)로 그대로 그린 것(16 px 칩, 32 px 타일 = 2×2 칩 칸). **규칙 층** = 칸마다의 지형 바이트(칩 통계가 아님). 행의 글자는 원작 지형 코드의 36진수(`0`–`9`, `a`–`h`, FORMATS §10.4 표 그대로)이고 `legend`가 기본 팩 지형 id를 정함(코드 10 문만 원작 모드 지형 `closed_gate`, 지형 규칙 행). 팩 지형이 없는 코드(화염·탁류, 문서에 없는 코드)는 **그 칸의 칩이 다른 맵들에서 가장 많이 쓰인 지형**으로, 맵 밖을 뜻하는 코드 255(실물에서는 맵 32의 한 칸)는 원작에서 이동으로 들어갈 수 없으므로 절벽으로 대신하고 `original-pack.json`과 맵 파일 주석에 기록. id의 번호는 시나리오가 맵을 가리키는 번호. `name`은 이름 항목의 원문(`신야1`처럼 숫자 포함) |

* **한계**: 서장·1장 전투의 소속 변경(`set_country`, 산적 두목 영입)은 기본 팩 이벤트가 맡고(2–4장 전투의 것은 캠페인 합류 플래그로
  옮김, DECISIONS D18), 원작 전투 중 곡 바꾸기는 옮기지 않으며 전투 파일 주석에 적습니다. 조건이 맞지 않을 때의 분기(하비 적교 칸에서 세 장수를 만나기 전 유비의 대사)는 이벤트의 `unless`로,
  전투 중에 바뀌는 목표 문구는 `set_objective`로 옮깁니다(번호와 `[이름]` 표시를 떼어 한 줄로). 기본 팩 이벤트가 맡지 않는 일기토(하비의 세 장수 등)는 원작 대사 사이에 원작 기마 그림으로 된 일기토 장면(`@duel`, FORMATS §13.6)으로 보여 줍니다(배경은 원작처럼 무장이 선 칸의 지형: `@duel … terrain`과 지형마다의 `gfx/duel/terrain_<지형>.png`). 기본 팩 이벤트 가운데
  기본 맵의 칸이나 원작에 없는 무장·증원 그룹을 쓰는 것은 빠지고, 그 대사 장면은 원작 대사가 대신합니다. 원작 AI 방식은
  `MAIN.EXE`의 AI 코드 이름(대기·최단 적공격·부동·이동·무공격이동)대로 옮깁니다(FORMATS §13.4). 위의 "전투" 행(기본 팩의 시험 전투를 다시 짠 것)은 원작 캠페인이 틀지 않지만,
  체인에 남은 기본 팩 전투가 원작 규칙과 맵으로도 맞도록 계속 만듭니다(그래서 검증이 "is not used by any campaign battle node" 경고 하나를 냅니다, D21). 서장·1장은 "원작 캠페인" 행의 전투(`c0_…`·`c1_…`)로 플레이하며, 계교 전투의 적 명단은 원작이 거록을 거쳤는지(플래그 133)로 고르는데 장 변환은 플래그 없이 읽어 청하 루트의 명단으로 고정됩니다(BACKLOG). 전투 장면 연출은 엔진에 없어 원작의 전투 장면 그림(`HEXBCHR`)은 쓰지 않고, 일기토만 기마 그림(`HEXICHR`)으로 보여 줍니다. 서장·1장 밖의 원작 맵은 2–4장 전투가 씁니다.
* **공유 금지**: 팩 안의 그림은 원작 데이터에서 변환한 것입니다. `data/original/`은 `.gitignore`에 있으며, 자기 PC에서만
  쓰세요.

## 5. 매니페스트 공유로 돕기 (특히 Steam판)

Steam판(2017, 앱 628150)은 지금 새로 살 수 있는 유일한 판본이지만, 설치 폴더 안의 컨테이너 형식이 알려져
있지 않습니다. 보유자가 매니페스트를 공유해 주면 저작물 없이도 형식을 파악할 수 있습니다.

1. `hero-tools original probe "<Steam>/steamapps/common/Eiketsuden1" --out steam-manifest.json`
2. 파일을 열어 **파일 목록을 직접 확인**하세요. 매니페스트에는 폴더 기준 **상대 경로**, 크기, SHA-256,
   각 파일의 **첫 16바이트**(형식 식별용 매직 넘버), 컨테이너 요약(LS11 항목 수·저장/복원 길이·검증 결과,
   6바이트 테이블 항목 길이, PC-98 디스크 이미지 헤더 종류, `MAIN.EXE` 팔레트 위치)만 들어갑니다.
   복원된 데이터·텍스트·픽셀과 절대 경로(사용자 이름이 들어가는)는 들어가지 않습니다.
3. 게임 폴더가 아닌 곳(예: 홈 폴더)을 가리키면 그곳의 파일 이름이 기록되니 주의하세요. 최대 5,000개 파일,
   8단계 깊이까지만 보며 심볼릭 링크는 따라가지 않습니다.
4. 프로젝트 저장소의 이슈에 매니페스트를 첨부해 주세요. 한국어 DOS/V·번체 중문판·PC-98 이미지의 매니페스트도
   변형판 식별에 도움이 됩니다.

## 6. 검증 방식

* **구조 불변식을 오라클로**: LS11 디렉터리 체인·파일 끝 일치·정확한 복원 길이·입력 완전 소비, 6바이트 테이블
  체인, 메시지 표(첫 값 = 2 × 섹션 수), `SNRnM` 섹션 수 = `SNRnD` 장면 수, 스크립트가 가리키는 대화·문자열이
  섹션 안에서 끝나는지, 블록·레코드·명령 길이. 위반하면 그
  종류는 정확한 오류와 함께 실패하고 부분 결과를 남기지 않습니다.
* **합성 픽스처**: 우리 LS11 인코더·6바이트 테이블 작성기·플레인 인코더·팔레트 뱅크 작성기로 만든 데이터로
  왕복·손상 입력·불변식 위반을 단위 테스트합니다.
* **골든 테스트** (`crates/hero-import/tests/golden.rs`): 자기 설치본의 데이터 폴더를 가리키면 실행됩니다.

  ```sh
  EIKETSU_ORIGINAL_DIR="D:/Games/영걸전/GAME" cargo test -p hero-import --test golden -- --nocapture
  ```

  모든 컨테이너 검증, 한국어판 공개 수치(얼굴 240·`PACKGRP` 38·`HEXBCHR` 181개와 데이터 시작 0x990,
  전투 맵 59개와 0번 56×32, 캠페인 맵 크기 공식, 장면 수 1/5/4/5/3, 프롤로그 이벤트 오프셋 표, `SNR0M`
  10,920바이트·`IPPAN0M` 37,580바이트·문자열 653개, 메시지 섹션의 모든 바이트가 스크립트로 덮임, 팔레트
  오프셋 0x38DF0), 전체 추출 성공을
  확인합니다. 같은 검사를 문서화된 형태로 만든 합성 설치본에도 돌려 CI에서 검사 코드 자체를 검증합니다.
  공개된 얼굴 0번 해시 접두어는 출력 배치가 문서화되지 않아 비교할 수 없어 검사하지 않습니다. 얼굴·`PACKGRP`
  디코딩은 `tests/tfdce_golden.rs`가 따로 확인합니다(240개 64×80·2560바이트·플레인 3 = 0, 38개 크기 목록).
  실물에서만 도는 `golden_korean_scenario_facts`는 FORMATS §11–§14의 수치(블록·레코드·명령·대화·문자열 수, `SNR3M`
  기준 넘침, 서장 두 전투의 턴·격파 목표·조건부 우군·적장 병종/레벨, `IPPAN0` 조각, 공개 능력치)를 확인합니다.
  골든 테스트는 주제별로 나뉘어 있어(`golden_korean_ls11_archives`, `_table_containers`, `_map_geometry`,
  `_maps`, `_scenario_text`, `_scenario_facts`, `_palette`, `_sprites`, `golden_every_container_validates`,
  `golden_extraction_succeeds`)
  실패하면 어느 형식이 틀렸는지 이름으로 드러납니다. 검증한 사본에서 손상된 파일은 SHA-256으로 기록해
  (`KNOWN_DAMAGED`) 정확히 기록된 오류로 실패하는지와 손상 전 항목이 복원되는지만 확인합니다.
* **실물 검증 현황 (2026-09, 한국어 DOS/V 사본 1개)**: LS11 코덱·두 디렉터리 바이트 순서, 판본 식별,
  팔레트 뱅크, 스프라이트·칩·전투 UI 배치, 6바이트 테이블(`FACEDAT`/`PACKGRP`)과 TF-DCE 디코딩(얼굴 240개,
  `PACKGRP` 38개), 전투·전투 장면·캠페인·도시 맵([FORMATS §10](reverse-engineering/FORMATS.md#maps))은 실물로 통과했고 PNG를 눈으로 확인했습니다.
  메시지·시나리오·`IPPAN0`·`BAKDATA`(10절)도 실물로 통과했고, 무장 이름 ↔ 얼굴 대응은 한 장에 그려 눈으로
  확인했습니다. 모든 골든 테스트와 전체 추출(모든 종류 `extracted`)이 실물에서 통과합니다.

## 7. 로드맵 (정직한 현황)

| 단계 | 범위 | 현황 |
|---|---|---|
| P0 프로브 | 판본 식별, 공유용 매니페스트 | **완료** (Steam·PC-98은 식별만) |
| P1 컨테이너 | LS11(+인코더), 6바이트 테이블 | **완료**. 음악용 테이블 컨테이너는 P7과 함께 |
| P2 텍스트 | `SNR?M`, `IPPAN0/IPPAN0M` (EUC-KR/Big5) | **완료** (화자별 대사 줄, 마을 사람 대사까지; 실물 검증) |
| P3 그래픽 | 플레인 셀·팔레트 → PNG | **부분**: 스프라이트·칩·배경 셀·전투 UI 아이콘 완료(실물로 확인), 얼굴(TF-DCE) 완료. `PACKGRP` 화면은 추출 종류는 없지만 원작 모드 팩이 화면 틀 3개와 사건 삽화 31개로 변환(16색 그림 3개의 팔레트 슬롯은 추론). 오프닝/엔딩(`NPK016`)·`MARK`·`SSCCHR`는 미지원 |
| P4 맵 | `HEXZMAP` 59개, `HEXBMAP`, `MMAP`, `SMAP`/`PMAP` → 타일 맵 JSON + 참고 PNG | **완료**(실물로 확인, [FORMATS §10](reverse-engineering/FORMATS.md#maps)). 남은 것: 맵별 팔레트 슬롯(조사 중, [STATUS 2절](reverse-engineering/STATUS.md#2-부분-해독)), `SMAP`/`PMAP` 물체 id의 의미 |
| P5 규칙·무장 | `BAKDATA` 배치 규명, `MAIN.EXE` 병종·지형·책략 표 서명 검색 | **부분**: `BAKDATA` 완료([FORMATS §14](reverse-engineering/FORMATS.md#bakdata)). `MAIN.EXE`의 지형 규칙(이동 비용·지형 효과)과 병종 규칙(공격·방어 계수, 이동력, 공격 범위) 완료, 병력 식과 책략 표(MP·도달·습득 레벨)·책략 위력·회복량·효과 범위와 원작 책략 식(`strategy_formulas`), 회복 아이템 회복량, 사기가 떨어질 때의 혼란과 오를 때의 풀림 굴림까지 완료. 남은 것: 스크립트 명령 `2D`(한 편 전체 사기 절반)의 사용처. 무장은 통솔·무력·지력·병종·레벨·장비가 원작 값이고 원작 캠페인에서 합류하는 원작 무장이 더해지며, 원작 전투의 일반 유닛은 그 인물의 능력치로 싸운다(4.5절) |
| P6 시나리오 | `SNR?D` 바이트코드 → 우리 이벤트 형식으로 변환 | **부분**: 해독·JSON 추출 완료([FORMATS §13](reverse-engineering/FORMATS.md#scenario), 명령 일부 미확인). 서장~4장의 전투(배치·명단·보물·목표 칸·증원과 전투 중 이벤트·대사·일기토, 두 맵에 걸친 장판파·와구관은 전투 둘, 장판파는 백성 호위 포함)와 이야기(마을·캠페인 맵 블록의 대사 장면, 선택지·루트 분기·원작 플래그·전투 중 설득·패배 후 진행, 엔딩 4개)를 원작 모드 캠페인으로 변환(4.5절, DECISIONS D18·D21. 서장·1장은 실물 변환 확인, 플레이 확인 전). 기본 팩 서장·1장 전투 21개도 원작 맵 위로 다시 짬(캠페인은 쓰지 않음). 루트마다 다른 출진 설정·명단은 루트별 전투(D23). 전투 중 도착하는 아군은 출진 설정이 남겨 둔 칸에. 남음: 장 전투의 개막 대사 |
| P7 음악 | OPL2 시퀀스 → FM 합성 | **완료(CLI)**: 재생기 `SBOPL2.COM` 이식 + 자체 OPL2 에뮬레이터로 10곡을 `bgm/<키>.wav`로([FORMATS §16](reverse-engineering/FORMATS.md), DECISIONS D16). 게임은 시작 뒤 백그라운드로 렌더링해 붙이고, 곡 배정은 청취 확인 전 [추론]. 인트로는 한 번 틀고 루프를 반복(WAV `smpl` 청크, 전환 간격은 청취 측정 전) |
| P8 Steam / PC-98 | Steam 컨테이너(매니페스트 수집 후), 디스크 이미지 리더, Shift-JIS·OPN 변형 | 미착수 — **Steam 매니페스트가 선행 조건**. 암호화가 있으면 법률 검토 전 중단 |
| P9 세이브 | `ESAVE/MSAVE` 가져오기 | 선택 사항 |
| 원작 모드 팩 | 변환물을 기본 팩 키로 옮긴 레이어드 팩(8절) | **부분**: 얼굴·유닛 시트·32 px 지형 타일셋·원작 전투 맵 58개(그림 층 + 규칙 층), 서장~4장의 전투 66개(서장·1장 21개는 기본 팩 전투를 다시 짬)와 전투 중 이벤트, 2–4장 이야기와 캠페인, 규칙 표(지형·병종·책략·아이템·게임 식), 화면 틀(전투·캠프·상태 창)과 사건 삽화, 음악(4.5절). 남은 일은 [STATUS 4절](reverse-engineering/STATUS.md#4-플레이-가능한-원작-모드까지-남은-단계) |

## 8. 원작 모드 (부분 구현)

OpenRCT2가 RCT2 데이터로 게임을 보여 주듯, 장기 목표는 플레이어가 보유한 원작의 에셋으로 게임을 그리는
**원작 모드**입니다. 원작 모드는 별도 실행 경로가 아니라 **기본 팩을 확장하는 레이어드 팩**으로 설계합니다
(팩 레이어링은 [MODDING.md](MODDING.md#layered-packs-extends), 결정 기록은 [DECISIONS.md](DECISIONS.md) D8).

* **토대 (구현됨)**: `pack.toml`의 `extends = "../base"`로 팩이 다른 팩 위에 얹힙니다. 자식 팩은 자기가 가진
  파일만 적고, 규칙 파일·무장·캠페인은 자식이 적으면 부모 것을 대체하며, 전투·대사 장면은 합쳐지고(같은 id는
  자식이 우선), 미디어는 자식 폴더를 먼저, 없으면 부모 폴더를 찾습니다. `[presentation] canvas = [w, h]`로 팩의
  가상 캔버스 크기를 적을 수 있습니다(기본 480×270, 480×270..1280×800, 자식이 적지 않으면 상속).
* **구현됨 (4.5절)**: `hero-tools original pack`이 사용자의 정품에서 읽은 결과를 기본 팩 옆의 로컬 폴더
  `data/original/`(`.gitignore`에 등록)에 **팩**으로 씁니다. 그 `pack.toml`은 `extends = "../base"`와
  `[presentation] canvas = [640, 400]`(원작 화면 틀의 크기)을 적고, 변환에 성공한 것만 담습니다: 무장 얼굴, 19병종의
  유닛 시트(32×32), 원작 전투 맵에서 학습한 32 px 지형 타일셋(기본 팩의 시험 전투도 원작 맵 그림으로 다시 짜여
  있어, 그 칸들은 원작 맵이 없는 전투(원작 모드 위에 모드가 더한 기본 맵 전투)나 전투 변환·맵 그림을 읽지 못해
  대신 그릴 때만 쓰임. `tile_size`(32)는 그림 층 크기 확인에 늘 쓰임), 원작 전투 맵 58개(맵 파일: 칩 격자 = 그림 층,
  지형 격자 = 규칙 층, [DECISIONS.md](DECISIONS.md) D9)와, 그 위에서 원작대로 짠 전투·이야기·캠페인, 병종·지형·책략·아이템 규칙,
  화면 틀·삽화, 음악(아래와 4.5절 표). 변환하지 못한 종류는 체인을 통해 기본 팩에서 옵니다. 플레이어는 게임 안에서 원작 폴더를 고르고, 게임이 실행할 때마다 같은 팩을
  메모리에서 만들어 씁니다(4.1절, [DECISIONS.md](DECISIONS.md) D10). 파일로 쓴 팩은 `eiketsuden --data data/original`로
  실행합니다(개발·검증용).
* **구현됨**: 기본 팩의 시험 전투를 원작 전투의 맵·배치·명단·보물·증원·전투 중 이벤트로 다시 짠 전투 파일과
  그 대사 장면([DECISIONS.md](DECISIONS.md) D11·D12, 원작 캠페인은 쓰지 않음), 서장부터 원작의 엔딩 4개까지 원작의 전투와 마을·캠페인 맵 이야기로 만든 캠페인
  (D18·D21, 서장·1장은 실물 변환 확인, 플레이 확인 전), 원작의 병종·지형·책략·회복 아이템 규칙과 책략·사기 식(D19), 무장의 원작 능력치와 원작 캠페인에서 합류하는 원작 무장(D21), 전투·캠프 화면 틀과 상태 창(D15),
  일기토 연출(D17), 음악(D16, 게임이 시작 뒤 렌더링), 사건 삽화(4.5절).
* **아직 없는 것**: 허창 1의 루트별 도착(BACKLOG), 전투 이벤트 스크립트의 삽화,
  16색 그림의 팔레트 슬롯 확정, 곡 배정과 인트로 → 루프 전환의 청취 확인([BACKLOG](../BACKLOG.md)). 매핑 규칙이 정해지지 않은 것은 추측해서
  넣지 않고, 규명되는 순서대로 팩에 들어갈 항목이 늘어납니다([STATUS 4절](reverse-engineering/STATUS.md#4-플레이-가능한-원작-모드까지-남은-단계)).
* **제약**: `extends`는 상대 경로만 허용하므로(웹 빌드와 폴더 이동을 위해) 파일로 쓴 원작 모드 팩은 기본 팩과 같은
  드라이브, 예컨대 `data/original/`에 둡니다. 게임이 메모리에서 만든 팩은 기본 팩 옆(`<data>/original`)에 있는 것처럼
  마운트되므로 이 제약과 무관합니다. 브라우저는 로컬 폴더를 읽을 수 없으므로 웹 빌드에는 원작 모드가 없습니다.
  세이브는 최상위 팩의 `id`를 기억하므로 원작 모드의 세이브는 기본 팩의 세이브와 섞이지 않습니다.

## 9. 형식 요약 (그래픽·컨테이너·맵)

형식 사실의 **전체 명세는 [reverse-engineering/FORMATS.md](reverse-engineering/FORMATS.md)**로 옮겼습니다. 분석 방법은
[METHOD.md](reverse-engineering/METHOD.md), 해독 현황은 [STATUS.md](reverse-engineering/STATUS.md)에 있습니다.
아래는 사용자가 알아 두면 좋은 요점입니다(한국어 DOS/V 실물 1개로 확인).

* **판본** ([FORMATS §3](reverse-engineering/FORMATS.md#edition)): 한국어판 `DISK1.R3I`에는 일본어 DOS/V 헤더(Shift-JIS)가 그대로 있고
  대사만 EUC-KR 한글이라, 헤더와 텍스트 통계를 함께 봅니다.
* **컨테이너** ([§4](reverse-engineering/FORMATS.md#ls11), [§5](reverse-engineering/FORMATS.md#table6)): 매직 `LS11`은 디렉터리 빅엔디언, `Ls11`은 리틀엔디언입니다.
  24개 중 23개가 완전히 통과하고, 검증한 사본의 `OPGRP.R3`는 약 0x60400부터 손상되어 있습니다(골든 테스트에 해시로 기록).
* **팔레트** ([§7](reverse-engineering/FORMATS.md#palette)): `MAIN.EXE` 안의 9 슬롯 × 48바이트 `[B][R][G]` 뱅크를 서명으로 찾습니다. 게임은 슬롯을
  실행 중에 데이터로 고르므로 추출기는 눈으로 확인한 슬롯(대부분 1, 도시 0, 궁궐 2, 얼굴 0)을 씁니다.
* **스프라이트·칩** ([§8](reverse-engineering/FORMATS.md#planar)): 16×16 셀, 4 플레인, 행 우선. 아카이브별 배치와 항목 묶음(19병종·효과·
  기마 무장)은 `sprites.json`에 기록됩니다. `HEXGRP` 0번은 packed planar입니다.
* **미해독** ([§9](reverse-engineering/FORMATS.md#opening)): 오프닝·엔딩의 `NPK016` 그림, `MARK.R3`, `SSCCHR1/2.R3`.
* **맵** ([§10](reverse-engineering/FORMATS.md#maps)): 전투 맵은 `[W][H][칩 W×H][지형 (W/2)×(H/2)]`이고 칩 뱅크는 `HEXZCHP` 0번 + 1번 또는
  2번(어느 쪽인지는 `MAIN.EXE`의 목록). 지형 코드 20개, 전투 장면 띠, 캠페인 맵 행군로, 도시·궁궐 보행 격자까지 해독했고,
  `MAIN.EXE`의 표는 모두 그 표를 읽는 코드로 찾습니다([§15](reverse-engineering/FORMATS.md#main-exe)).

## 10. 대사·시나리오·마스터 데이터 요약

전체 명세: 메시지 [FORMATS §11](reverse-engineering/FORMATS.md#text), `IPPAN0` [§12](reverse-engineering/FORMATS.md#ippan), 시나리오 바이트코드
[§13](reverse-engineering/FORMATS.md#scenario), `BAKDATA` [§14](reverse-engineering/FORMATS.md#bakdata).

* **메시지** `SNRnM`: 섹션 기준 표 + 섹션. 섹션 안은 대화(`[u16 화자][텍스트]00 … FFFF`)와 평문 문자열이 섞여 있고
  경계는 바이트코드만 압니다. 64 KiB를 넘는 `SNR3M`의 기준은 넘침을 복원합니다.
* **시나리오** `SNRnD`: 장면 → 블록 → 10바이트 트리거 레코드 → 스크립트. 명령 `0x00`–`0x3D`의 피연산자 길이는
  `MAIN.EXE` 인터프리터에서 정했고, 일부 명령은 의미가 미확인입니다.
* **마을 사람 대사**: `IPPAN0`(장별 색인)이 `IPPAN0M`(문자열 653개)을 가리킵니다.
* **`BAKDATA`**: 마을 사람 256, 아이템 64, 무장 384 + 무장 초기 상태 384. 능력치 순서는 통솔·무력·지력, 얼굴 번호는
  `FACEDAT` 항목입니다.

**추출 결과**: `--text`는 `text/snr<n>.json`(장면 → 섹션의 대화·문자열 목록과 블록 → 레코드 → 명령, 명령마다 이름·텍스트를
풀어 쓴 `resolved`), 같은 내용의 읽기용 `text/snr<n>.txt`, `text/ippan0m.json`, `text/townsfolk_talk.json`,
`text/officers.json`·`items.json`·`townsfolk.json`, 장마다 이야기 흐름 개요(원문 포함) `text/flow<n>.md`와
원문을 뺀 전체 개요 `text/scenario_structure.md`(읽는 법: [reverse-engineering/SCENARIO.md](reverse-engineering/SCENARIO.md))를
씁니다. 실물 사본에서는 18장면, 명령 14,404개, 대화 2,619개, 문자열 901개, 마을 사람 대사 653개, 디코딩 실패
0으로 추출됩니다. 게임의 이벤트·드라마 형식으로 바꾸는 일은 원작 모드 팩이 합니다(4.5절, DECISIONS D18).

---

## English summary

The **original-data importer** is an optional, experimental feature for players who **own** a copy of KOEI's
1995 *Sangokushi Eiketsuden*. The game never needs it. It reads the player's install **read-only**, uploads
nothing, never circumvents copy protection (an encrypted container stops at detection), and writes neutral files
(PNG, UTF-8 JSON) into a local folder the player chooses (never inside the install; `data/original/` is
git-ignored). It is a clean-room implementation from format facts verified on an owned copy; no third-party code
was used, and the repository and CI contain no original bytes (tests use synthetic fixtures from our own encoders).

* **Format documentation**: the full, verified format specification now lives in
  [reverse-engineering/FORMATS.md](reverse-engineering/FORMATS.md) (edition identification, `LS11`/`Ls11`
  archives, 6-byte tables, TF-DCE, palettes, planar sprites, maps, message files, scenario bytecode, `IPPAN0`,
  `BAKDATA`, the `MAIN.EXE` code signatures), with the method in
  [METHOD.md](reverse-engineering/METHOD.md) and the open work in [STATUS.md](reverse-engineering/STATUS.md).
  Sections 3.1, 9 and 10 of this page are short summaries of it.
* **Editions**: Korean DOS/V (the `DISK1.R3I` header is the Japanese DOS/V one, so the Korean EUC-KR text decides;
  extractable and verified), Traditional-Chinese DOS (DOS/V family + Big5 text, medium confidence, extractable but
  not yet verified on a real copy), Steam 2017 and PC-98 disk images (identified only), anything else `unknown`
  (refused unless `--edition` is given).
* **Extracted from the verified Korean copy**: palettes (`MAIN.EXE`, slot chosen at run time by the game, so a
  visually checked slot is used per archive), unit sprites, map chips and battle UI icons with contact sheets and
  `sprites.json`, the 240 TF-DCE portraits, battle/scene/campaign/town/palace maps with terrain codes
  (`--maps`), scenario scripts with dialogue and strings (`text/snr<n>.json`/`.txt`), townspeople lines and the
  `BAKDATA` officer/item/townspeople tables. `PACKGRP.R3` screens decode but are not an extraction kind (the original mode pack converts them). Not
  decoded: `NPK016` opening/ending pictures, `MARK.R3`, `SSCCHR1/2.R3`; the owner's `OPGRP.R3` is damaged.
* **Usage**: `hero-tools original probe <dir> [--out manifest.json]`, then
  `hero-tools original extract <dir> --out <overlay> [--text] [--sprites] [--portraits] [--maps]`, then
  `eiketsuden --original <overlay>` (or `EIKETSUDEN_ORIGINAL`; native builds only). Media keys are looked up in
  the overlay first, then in the pack. The extracted keys live under `original/...` and do not replace the base
  pack's own keys automatically yet. The overlay replaces images, sounds, the four index files (`icons.toml`,
  `units.toml`, `terrain.toml`, `fx.toml`), each file on its own (an overlay `credits.txt` is shown before the
  packs'): sheets with other frame
  or tile sizes need their index in the overlay too, and an overlay index replaces the pack's as a whole.
  `extract` writes no index files; the original mode pack (section 4.5) is the usual way to play with the
  original art.
* **Help wanted**: run `probe` on a Steam install (`steamapps/common/Eiketsuden1`) and attach the manifest to an
  issue. A manifest contains relative paths, sizes, SHA-256, the first 16 bytes of each file and container
  summaries — no game content, no absolute paths.
* **Verification**: `EIKETSU_ORIGINAL_DIR=<data folder> cargo test -p hero-import --test golden` checks the
  published known answers on a real install. The golden tests are split by topic; on the verified copy all of them pass —
  LS11, edition, 6-byte-table, palette, sprite, map-geometry, map (`golden_korean_maps`), scenario text and facts,
  and the full extraction — as do the TF-DCE checks in `tests/tfdce_golden.rs`.
* **Roadmap**: P4 maps done (per-map palette slot and the town object ids open); P5 mostly done (`BAKDATA`
  decoded; the `MAIN.EXE` rule tables converted, ability values stay the base pack's); P6 mostly done (bytecode
  decoded and extracted; the battles of the prologue and chapters 1–4 and the story of chapters 2–4 converted to
  the game's event and drama formats); P7 OPL2 music done (rendered to WAV, DECISIONS D16); P8 Steam/PC-98 open.
* **Original mode in the game (no command line)**: on native builds the title menu's "원작 데이터" (original data)
  opens an in-game folder browser (★ marks folders holding original files; entering one shows the same verdict and
  evidence as `probe`; a folder whose only install is one subfolder, like a DOSBox package, offers that subfolder;
  "경로 입력…" or Ctrl/Cmd+V takes a typed or pasted path). "이 폴더 사용" (use this folder) stores the folder in
  the settings and reloads: the loading
  screen loads the base pack, converts the install **in memory** on a worker thread (about 0.1 s in a release build
  on the Korean copy), mounts the result next to the base pack and plays it; nothing is written and the install is
  only read. Later launches start in the original mode directly; the same screen switches back to the base pack.
  A missing folder, an unsupported edition or a failed conversion ends on an error screen that offers retry, another
  folder, or the base pack. An explicit `--data` / `EIKETSUDEN_DATA` wins over the setting. See DECISIONS D10.
* **Original mode pack as files (development)**: `hero-tools original pack <dir> --out data/original` writes the same
  layered pack (`id = "original"`, `extends` the base pack, `canvas = [640, 400]`, git-ignored) holding what can be mapped
  onto the base pack's keys, then validates it; play it with `eiketsuden --data data/original`. `hero-tools validate` and the game
  warn when the base pack (or the converter's pack format) changed since it was written (`base_fingerprint` in
  `original-pack.json`), also for a mod that extends it. It holds officer
  portraits (matched to the base pack's officers by name, with five spelling aliases and one reading used to tell
  two officers of the same name apart; 108 of 118 on the verified copy), unit sheets of all 19 classes from the
  `HEXZCHR` map icons (32×32 frames; orange for the player and allies, green for enemies, as `MAIN.EXE` picks
  them) and a 32-px terrain tileset learned from the 58 original battle maps (per terrain and
  neighbour mask the 2×2-chip block the maps show most often). It also holds the 58 original battle maps as a
  map file (`maps/original.toml`, ids `hexz_NN`): the chips as a picture layer (`gfx/maps/hexz_NN.png`) and the
  terrain bytes as the rules grid. The base pack's battles (now its one test battle; the prologue's and chapter 1's 21 before D21) are re-staged as the
  original battles (`battles/<id>.toml`, replacing the base battles): the original map, turn limit, deployment
  tiles, enemy and allied rosters (officers by name, otherwise generic units with the `BAKDATA` name; class,
  level and AI from the original), treasures, Liu Bei's objective tile and the units that join later with their
  arrival triggers, while names, the starting objective texts, scenes, music, rewards and the base events that
  still fit stay the base pack's (DECISIONS D11). The original's mid-battle events are converted too (DECISIONS D12): the
  battle block's trigger groups from 3 on are phases (a flagged group is watched in parallel until a script leaves
  parallel control), and each trigger record becomes an event with its trigger and actions (joins, AI changes,
  levels, retreats, gold and items, a gate opening or the Xiapi drawbridge coming down as `set_terrain` with the
  new chips' picture, the end of the battle, `set_stage` into the next phase); its dialogue, narration and duels
  become drama scenes written from the player's copy (`dramas/original_battles.drama`; duels as `@duel` scenes with the
  original's riders in `gfx/duel/`). Flags that one record sets
  and another tests become battle flags with `when` conditions (and what a script does while they do not hold,
  events with `unless`); objective texts a script changes become `set_objective`; where the base battle keeps an event for the same
  occasion (most duels), it keeps telling it and gains the original's other actions. The rules, the battle, camp and status frames and the music are the original's too (the game
  renders the music in the background after it starts). The campaign is the original's from the prologue
  (DECISIONS D18, D21; the prologue and chapter 1 checked on a real copy, not yet play-tested): chapters 2 to 4 have 43 battles (Changban and Wagu Pass, fought on two maps, are two battles each; Changban's
  people are allied civilians the opening marches to a village) and their story — towns and campaign-map blocks as
  dialogue scenes with choices, route branches, joins and the original's four endings. The officers take the original's
  stats, classes, levels and equipment, the persons who join that the base pack lacks are added, and the original
  battles' generic units fight with their persons' stats. The parts of the UI not listed above still come from the base pack. `extends` is relative only, so the pack lives next to the
  base pack; there is no original mode on the web. The remaining steps are listed in
  [STATUS.md](reverse-engineering/STATUS.md#4-플레이-가능한-원작-모드까지-남은-단계).

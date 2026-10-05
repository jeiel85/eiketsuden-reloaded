# 해독 현황과 남은 과제

> **English summary** — State of the reverse-engineering and of the original mode as of 2026-09-30 (one Korean
> DOS/V copy). Decoded and verified: edition identification, LS11/Ls11 containers, 6-byte tables, TF-DCE (240
> portraits, 38 `PACKGRP` screens), the `MAIN.EXE` palette bank, sprite/chip archives, battle/scene/campaign/town/palace
> maps with terrain codes, message files, the scenario bytecode, `IPPAN0` and `BAKDATA`, the `MAIN.EXE` rule tables
> (movement and terrain, class coefficients and troops, strategies, healing items, the strategy and morale formulas)
> and the OPL2 music (the `SBOPL2.COM` driver and the song files, rendered by our own OPL2 emulator). Partly decoded:
> which palette slot each screen uses (chosen at run time from data), several opcodes and trigger flags, town object
> ids, some `BAKDATA` bytes, the 16-colour `PACKGRP` palettes, which song plays where (not yet listened to). Not
> decoded: the `NPK016` codec and the opening/ending pictures and palettes, `MARK`/`SSCCHR`, saves, Steam and PC-98
> containers. Original mode: `hero-tools original pack` (and the game itself, at launch) converts the player's copy
> into a layered pack over the base pack: portraits, unit sheets, a tileset and the 58 maps, the rule tables, the
> battle, camp and status frames, event pictures, the music, all 60 of the original's battles (Changban and Wagu Pass,
> fought on two maps, are two battles each) with their in-battle events, dialogue, duels and Changban's escort of
> the people, and the story of chapters 2–4 up to the original's endings; a battle whose setup or rosters the
> original picks by route flags is one battle per route (D23). Allies arrive during a battle on the tiles the setup keeps for them. Remaining: the opening lines of chapter battles, the
> palette slots of the 16-colour pictures, listening to the song assignment, and formats the game needs no more
> (opening/ending pictures, saves).

형식의 세부는 [FORMATS.md](FORMATS.md), 사용자용 로드맵은 [../ORIGINAL_DATA.md §7](../ORIGINAL_DATA.md#7-로드맵-정직한-현황)에
있습니다. 기준: 한국어 DOS/V 사본 하나, 2026-09-30 갱신. 실물에서 모든 골든 테스트와 전체 추출(모든 종류 `extracted`)이 통과합니다.

## 1. 해독 완료 (실물 검증)

| 영역 | 파일 | 추출 결과 | 확인 방법 |
|---|---|---|---|
| 판본 식별 | `DISK*.R3I`, 텍스트 통계 | 프로브·`index.json` | 실물 판정 `korean-dos`, 근거 기록 |
| LS11 / Ls11 | `.R3` 24개 | — | 23개 모든 항목 정확 복원, `OPGRP`은 사본 손상으로 기록 |
| 6바이트 테이블 | `FACEDAT`, `PACKGRP` | — | 체인·파일 끝 일치 |
| TF-DCE | `FACEDAT` 240, `PACKGRP` 38 | 얼굴 PNG(`PACKGRP`는 디코딩만) | 입력 정확 소비, 눈으로 확인, 파이썬 원형과 해시 일치 |
| 팔레트 뱅크 | `MAIN.EXE` | `palettes.json` | 서명 탐색, `[B][R][G]` 실험 |
| 스프라이트·칩·전투 UI | `HEX*CHR`, `HEX*CHP`, `*BGPL`, `HEXGRP` 0 | PNG, 시트, `sprites.json` | 컨택트 시트 |
| 전투 맵 58개 + 이름 | `HEXZMAP` | `maps/battle*.json`, PNG | 뱅크 규칙을 로더 코드로 확인, 이음매 없음 |
| 지형 코드 20개 | `MAIN.EXE` 이름 표 | `battle.json` | 칸 그림·배경 표와 교차 확인 |
| 전투 장면 띠 | `HEXBMAP` | `scene.json`, PNG | 그림 |
| 캠페인 맵 4개 | `MMAP` + 크기 표 | `campaign.json`, PNG | 행군로가 길과 겹침 |
| 도시 12 / 궁궐 23 | `SMAP`, `PMAP` | `town.json`, PNG | 보행 격자가 길과 겹침 |
| 메시지 | `SNR0M`–`SNR4M` | `text/snr<n>.json`·`.txt` | 모든 바이트 덮임 |
| 시나리오 바이트코드 | `SNR0D`–`SNR4D` | 같은 파일(해독된 명령과 `resolved`), 흐름 개요 `flow<n>.md`·`scenario_structure.md`([SCENARIO](SCENARIO.md)) | 모든 바이트 설명, 서장 흐름이 공개 공략과 일치 |
| 마을 사람 대사 | `IPPAN0`, `IPPAN0M` | `townsfolk_talk.json`, `ippan0m.json` | 653개 빠짐없이 참조 |
| 마스터 표 | `BAKDATA` | `officers/items/townsfolk.json` | 공개 능력치, 이름-얼굴 대조표 |

## 2. 부분 해독

| 항목 | 아는 것 | 모르는 것 / 다음 단계 |
|---|---|---|
| 화면별 팔레트 슬롯 | 팔레트는 전역이라 그림마다 따로 고르지 않음. 설정 루틴(이미지 0x118CE)을 부르는 감싸개는 셋(0x11946·0x11958·0x11960). 상태 창(0x2F4EB)과 전투(0x208B3 등)는 게임 상태 구조체 DS 0x7A6C의 +5 하위 4비트(`0xcf5:0x21EA`)를 씀, 즉 **그때의 장소 슬롯** **[코드]** | 어느 레코드가 0x7A6C를 채우는지. +5를 쓰는 곳 후보: 이미지 0x14061·0x13897·0x1693C·0x16A4D·0x17C31·0x20632(0x14346은 마을 사람 표 256 × 13 B의 +8을 쓰는 다른 구조체 로더). 지금 원작 모드는 전투 틀 = 맵 슬롯, 상태 창 = 슬롯 0으로 둔 추론 |
| `PACKGRP` 16색 그림(1·2·37번) | 디코딩 완료 | 맞는 팔레트 슬롯. 추출 종류로도 아직 없음(CLI·`index.json` 변경 필요) |
| 시나리오 명령 | 0x00–0x3D 전부 길이 확정 | 의미 미확정: `05` show_screen, `06`, `0C`, `16`, `17`의 값, `20` enable_list의 32바이트 표, `23`, `35`의 동작 코드, `36` begin_battle, `3B`/`3C`/`3D`(캠페인 맵 연출). `19`는 모순 길이로 거부(데이터에 없음) |
| 트리거 | 12종의 판정과 인자 대부분, 전투 블록의 그룹 = 단계와 그룹 플래그 = 병행 제어(FORMATS §13.2) | 판정 반전 비트(0x80)의 정확한 의미, 종류 2의 b 인자, 종류 5, 전투 밖 블록에서의 그룹 플래그 |
| 전투 명단 블롭 | 무장·좌표·조건·AI·병종·레벨, "나중에 합류" 바이트(슬롯 셋째·명단 둘째), AI 방식 0–6(내부 코드와 이름, FORMATS §13.4) | 나머지 `?` 바이트 |
| 좌표·맵 번호 | 맵 번호 상위 니블 = 종류, 전투(3)의 하위 바이트 = `HEXZMAP` 항목. 명단·슬롯 좌표는 열이 먼저, 트리거 레코드의 칸은 행이 먼저(FORMATS §13.2·§13.4) | 하위 바이트 → `MMAP`/`SMAP`/`PMAP` 항목 대응, 서장과 1장이 `MMAP` 0번을 함께 쓰는지 |
| `SMAP`/`PMAP` 물체 | `(id, x, y)` 구조, 보행 표시 지점 값 | id의 뜻(인물·장식, `SSCCHR` 관련?), 0x7F/0xFF 이외 표시 값의 뜻 |
| `BAKDATA` | 네 표의 배치, 능력치·얼굴·소속·병종·아이템 | 역할 바이트, 무장 플래그 바이트, 스프라이트 번호가 가리키는 그림 |
| `IPPAN0` | 배치와 기본 그룹(키 125) | 125 이외 키가 비교하는 상태 값, 마을 번호 ↔ 맵 번호 |
| 스프라이트 정체 | 병종 묶음, 효과 | `HEXICHR` 세트별 무장, `HEXZCHR` 43–44를 그리는 상태 `& 0x02`의 뜻(고르는 조건은 FORMATS §8.2 [코드]) |
| `MAIN.EXE` 이름 목록 | 장 제목 18, 소속 15, 병종 19가 있음 | Rust 추출기는 번호만 기록(이름 목록을 읽는 서명 미작성) |
| 칩별 지형 | 통계 표(`chip_terrain`) | 118개 경계 칩은 맵 격자를 따라야 함(변환기 설계에 반영) |
| `MAIN.EXE` 규칙 표 | 코드 서명으로 찾아 원작 모드 규칙 파일로 옮김: 병종 → 이동 종류, 이동 종류 × 지형 비용, 지형 효과, 병종의 공격·방어 계수·이동력·사거리·병력, 책략의 MP·도달·습득 레벨·위력·회복량·효과 범위, 회복 아이템, 책략·혼란·사기의 식(`strategy_formulas = "original"`, [FORMATS §10.4](FORMATS.md) [코드], `maps::find_move_rules`) | 스크립트 명령 `2D`의 사용처(위). 원작의 난수 함수 `rand(n)`(0xcf5:0x48C4, 이미지 0x11814)은 `0..n-1`로 확인 |
| 음악(OPL2) | 컨테이너(`MUSIC.R3` 20곡·`OPMUSIC` 2·`EDMUSIC` 3)와 재생기 `SBOPL2.COM`(시계·이벤트·악기, [FORMATS §16](FORMATS.md) [코드] [검증])을 해독하고, 자체 OPL2 에뮬레이터로 곡을 WAV로 렌더링(음 높이·감쇠·레벨은 시험) | 렌더링 결과를 귀로 확인한 적 없음. 곡 배정(`pack::MUSIC_KEYS`)은 시나리오 `0x38`의 문맥으로 고른 추론이고 전투 중 곡은 `int 65h` 경로(미확인). 트랙마다 다른 루프 지점(`FE`)과 인트로를 엔진이 쓰지 못함(파일째 반복) |

## 3. 미해독

| 항목 | 현재 상태 | 접근 방법 |
|---|---|---|
| `NPK016` 코덱(오프닝·엔딩) | 헤더·12비트 팔레트만 앎 | `OPEN.EXE`/`END.EXE`의 해제 루틴을 정적으로 읽기. TF-DCE 변형인지 먼저 확인 |
| 오프닝·엔딩 원시 그림 크기 | 행 간격 추정으로만 그려짐 | `OPEN.EXE`/`END.EXE`의 그리기 코드에서 너비·높이 찾기 |
| `OPEN.EXE`/`END.EXE` 장면 팔레트 | 위치만 앎(28 / 43 × 48 + 2) | 위와 함께 |
| 손상된 `OPGRP.R3` | 27–61번 복원 불가 | 다른 사본(매니페스트·해시)으로 교차 확인 |
| `MARK.R3`, `SSCCHR1/2.R3` | 배치 미해독 | 읽는 코드 찾기(파일 이름 문자열 참조부터), `SSCCHR1`을 배치표로 가정해 시험 |
| 세이브 `E*/M*.R3S` | 외부 주장만 | 선택 사항. `MSAVE` 무장 레코드 = `BAKDATA` 초기 상태 모양인지 확인 |
| 글꼴 | 미조사 | 필요 없음(자체 글꼴 사용) |
| Steam 2017 | 식별만 | 보유자의 프로브 매니페스트 수집이 선행. 암호화가 있으면 법률 검토 전 중단 |
| PC-98 디스크 이미지 | 헤더 식별만 | 이미지 리더(클린룸), Shift-JIS·OPN 변형 |
| 번체 중문 DOS판 | 합성 Big5 테스트만 | 실물 매니페스트·골든 실행 |

## 4. 플레이 가능한 원작 모드까지 남은 단계

원작 모드는 임포터가 사용자의 정품에서 변환한 결과를 **기본 팩을 확장하는 레이어드 팩**(`extends = "../base"`)으로
쓰는 방식입니다([../ORIGINAL_DATA.md §8](../ORIGINAL_DATA.md#8-원작-모드-부분-구현), [../DECISIONS.md](../DECISIONS.md) D8).

1. **팩 변환기** — 추출물을 팩의 키와 규칙으로 옮기는 매핑. **부분 완료**(`hero-tools original pack`,
   `crates/hero-import/src/pack.rs`, 사용법·매핑 규칙은 [../ORIGINAL_DATA.md §4.5](../ORIGINAL_DATA.md#45-원작-모드-팩을-파일로-만들기-개발검증용)).
   * 얼굴: **완료**. 기본 팩 무장 118명 중 108명(이름 대응 + 별칭 5 + 읽기 구분 1). 나머지 10명은 기본 팩이 새로 만든
     인물이거나 이 판본에 없는 이름.
   * 유닛: **완료**(맵 아이콘). `HEXZCHR` 19병종 × 두 색 → 32×32 시트와 `units.toml`. 짝수(주황) = 아군·우군,
     홀수(초록) = 적군([FORMATS §8.2](FORMATS.md#planar) [코드]). 무장 전용 아이콘(38–40, 45–46)과 혼란 아이콘(43–44, 상태 `& 0x02` = 혼란)은 원작 모드에서 씀. 전투 장면용 `HEXBCHR`/`HEXICHR`(48/64/96 px)는 엔진에 전투 장면
     연출이 없어 쓰지 않음.
   * 지형 타일셋: **완료**(학습). 원작 맵 58개에서 지형 × 이웃 마스크별 최빈 2×2 칩 칸을 골라 `tile_size = 32`의
     타일셋으로. 기본 팩 맵을 원작 칩으로 그릴 뿐이라 경계가 원작 맵만큼 매끄럽지 않음.
   * 원작 맵: **완료**(한국어 DOS/V 실물로 `golden_original_pack` 통과: 맵 58개, 대체 칸은 맵 32의 코드 255 하나, 맵 0 = 28×16 칸). 58개 맵이 맵 파일 `maps/original.toml`의
     `hexz_NN`으로: 칩 격자 → 그림 층 `gfx/maps/hexz_NN.png`, **지형 격자 → 규칙 층**(칩 통계가 아니라 맵의 지형
     바이트, 행 글자 = 지형 코드의 36진수). 엔진에 그림 층(`[map] image`)과 맵 파일(`[map] use`)을 추가했다
     ([../DECISIONS.md](../DECISIONS.md) D9). 팩 지형이 없는 코드(화염·탁류)는 그 칸의 칩이 가장 많이
     쓰인 지형으로 대신한다 **[추론]**. 맵 밖을 뜻하는 코드 255(맵 32의 한 칸)는 이동으로 들어갈 수 없으므로 절벽으로
     옮긴다([FORMATS §10.4](FORMATS.md) [코드]). 쓰는 전투는 2단계에서 생긴다.
   * 게임 안 연결: **완료**(이슈 #5). 타이틀 → "원작 데이터"에서 설치 폴더를 고르면 게임이 실행할 때마다 같은 팩을
     메모리에서 만들어 기본 팩 위에 얹는다(명령줄 불필요, [../ORIGINAL_DATA.md §4.1](../ORIGINAL_DATA.md#41-게임에서-원작-모드-켜기),
     [../DECISIONS.md](../DECISIONS.md) D10).
   * 원작 전투: **완료**(서장·1장). 기본 팩의 전투(지금은 시험 전투 하나, D21 전에는 서장·1장 21개)를 원작 전투의 맵·턴 제한·배치 칸·적과 우군
     명단(무장·칸·병종·레벨·AI)·보물·목표 칸·증원(`join_battle`)으로 다시 짠다(`crates/hero-import/src/battles.rs`,
     [../DECISIONS.md](../DECISIONS.md) D11). 전투 중 이벤트(일기토·턴과 영역에 따른 AI 전환·합류·대체 승리·성문과
     적교)도 원작 트리거 레코드에서 옮기며, 대사는 사용자 사본에서 대사 장면으로 만든다(D12). 기본 팩이 같은 계기로
     자기 말로 들려주는 이벤트(일기토 등)는 기본 팩 것을 쓰고 원작의 나머지 동작을 더한다. 조건이 맞지 않을 때의
     분기(하비 적교 칸에서 세 장수를 만나기 전 유비의 대사)는 `unless`로, 전투 중 목표 문구는 `set_objective`로 옮긴다.
     원작 일기토 연출은 `@duel` 장면으로, 그 배경은 무장이 선 칸의 지형으로 옮긴다(FORMATS §13.6, D17).
     소속 변경(`set_country`)은 서장·1장에서는 모두 기본 팩 이벤트가 맡고, 장 전투에서는 캠페인 합류로 옮긴다(D18).
   * 대사·무장·아이템: 전투 이벤트와 장 이야기의 대사는 사용자 사본에서 옮겼고(서장부터, D21), 회복 아이템의
     회복량은 원작 값(3단계). 무장의 통솔·무력·지력은 `BAKDATA` 값이고, 원작 캠페인에서 합류하는 원작 무장 중 기본 팩 무장이
     맡지 않는 사람은 `BAKDATA` 값으로 더한다(ORIGINAL_DATA 4.5절, D21). 병종·레벨·장비도 원작 값이고, 원작 전투의 일반 유닛은 그 인물의 능력치로 싸운다.
2. **시나리오 변환** — 해독한 바이트코드(트리거 그룹·명령)를 우리 이벤트·드라마 형식으로 옮기는 변환기(KOEI 바이트코드를
   실행 중에 해석하지 않음). **부분**: 서장·1장 전투 전체(배치·명단·보물·목표 칸·증원·전투 중 이벤트와 그 대사, 1단계의
   "원작 전투"), 서장~4장(`SNR0`–`SNR4`)의 전투(2–4장 45개. 출진 설정의 `battle_end`로 맵을 불러오는 4장 마지막 두 전투(이슈 #95) 포함. 전투 안의 `battle_end`가 다른 전투 맵으로 이어지는 장판파·와구관은 전투 둘로 나뉘고, 장판파는 출진 설정의 백성을 아군 유닛으로 호위)와 이야기(마을·캠페인 맵 블록의 대사 장면, 선택지·재질문·루트 분기·마을 이동·원작 플래그·전투 중 설득·패배 후 진행·엔딩 포함)로 만든
   원작 모드 캠페인(DECISIONS D18·D21, 서장·1장은 실물 변환 확인, 플레이 확인 전). 루트마다 다른 출진 설정·명단은 루트별 전투로(D23). 전투 중 도착하는 아군은 출진 설정이 남겨 둔 칸에(군의 무장이면 플레이어 쪽). 전투의 개막(그룹 2)은 전투 시작 이벤트로, 출진 설정이 보여 주는 삽화·서술·대사는 전투 앞 장면으로, 군 밖 무장의 레벨·병종 변경은 합류 때(D24). 한 편 전체의 사기·병력 절반(`2D`, FORMATS §13)은 이벤트 동작 `halve`로, 전투 이벤트의 `game_over`는 패배로. 남음: 전투 이벤트 스크립트의 삽화,
   남은 명령 의미(BACKLOG).
3. **규칙 표** — `MAIN.EXE` 규칙 표를 코드 서명으로 찾아 팩 규칙 파일로 변환(값은 사용자의 파일에서만 읽음). **부분**: 이동 비용·지형 효과 → 원작 모드 `rules/terrain.toml`(원작의 문은 닫힌 성문이라 기본 팩의 열린 `gate`와 따로 `closed_gate` 지형을 더한다. 기본 팩과 다른 값은 `original-pack.json`에 적는다). 병종의 공격·방어 계수·이동력·공격 범위·병력 → `rules/classes.toml`, 책략 MP·도달·위력·회복량·효과 범위와 병종별 습득 레벨 → `rules/strategies.toml`·`rules/classes.toml`(FORMATS §10.4 책략 효과), 원작 책략 식(지원 보너스, 혼란 명중·풀림, 사기가 30 미만으로 떨어질 때의 혼란과 오를 때의 풀림 굴림, 최소 피해) → `rules/game.toml`의 `strategy_formulas = "original"`(DECISIONS D19), 회복 아이템 회복량(병력 600/1200/1800, 사기 30/40/50) → `rules/items.toml`.
4. **원작 UI** — `PACKGRP`의 화면 틀(메인·전투 640×400, 상태 창 512×320)과 삽화를 쓰는 원작 배치의 UI. **부분**:
   전투 화면 틀 → 원작 모드 `[presentation.battle_frame]`(캔버스 640×400), 메인 화면 틀 → `[presentation.camp_frame]`(캠프
   화면들), 상태 창 → `[presentation.status_frame]`(캠프 무장 정보, DECISIONS D15). 전투 틀의 기능·아군·적군 버튼 → 전투 메뉴·부대 일람. 사건 삽화 31장 → `gfx/pictures/`, 장 이야기의 `@picture`. 남음: 메인 화면 아이콘 동작, 16색 그림(전투 틀·상태 창)의
   팔레트 슬롯 확정(§2).
5. **음악** — **부분**: 컨테이너·재생기 해독, 자체 OPL2 에뮬레이터, 원작 모드 팩의 `bgm/<키>.wav`(CLI는 팩에, 게임은 시작 뒤 백그라운드로 렌더링해 메모리 팩에, DECISIONS D16). 남음: 곡 배정 청취 확인, 게임이 전투 중 트는 곡(MAIN.EXE `int 65h` 경로), 인트로 뒤부터 반복하는 루프 시작점(엔진 오디오 변경 필요).
6. **오프닝·엔딩**(선택) — `NPK016` 코덱과 원시 그림 크기, 장면 팔레트.
7. **세이브 가져오기**(선택) — `ESAVE`/`MSAVE`.

각 단계는 [../ORIGINAL_DATA.md §7](../ORIGINAL_DATA.md#7-로드맵-정직한-현황)의 P4–P9와 대응합니다. 순서는 1 → 2 → 3이
게임을 돌리는 데 필요한 최소이고, 4·5는 원작다운 모습, 6·7은 선택입니다.

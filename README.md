# NeuroFour

저사양 컴퓨터에서 **동적 희소 신경망(dynamic sparse neural network)** 아이디어를 재현하고 측정하기 위한 의존성 없는 Rust 연구용 최소 구현입니다. 첫 환경은 Connect Four(사목)입니다. 규칙이 결정론적이고 상태가 작으며, 완전해결된 게임이라 공개 기보와 강한 참조 엔진을 붙이기 쉽습니다.

## 구현 범위

- 노드마다 `bias`, 출력 가중치, 이동평균 utility와 소수의 입력 연결만 보존
- 입력 연결의 미피복 비율(신규성)과 예측 오차가 동시에 임계값을 넘을 때 노드 생성
- top-k 절댓값 활성화만 출력과 학습에 사용
- 공동 활성화와 결과 오차 기여도를 연결 strength/노드 utility에 누적
- 오래 쓰이지 않고 약한 연결 가지치기, 빈/저효용 노드 휴면 및 제거
- random/heuristic/search 혼합 및 10% 탐험 수를 쓰는 고정 시드 데이터 생성
- 게임 단위 train/validation 분할, 좌우 반전 증강, validation 조기 종료
- 고정 시드의 학습 순서, 동률 선택과 텍스트 체크포인트
- random, 즉시 승/방어 heuristic, alpha-beta negamax search 기준선
- 선공을 번갈아 배정한 동일 게임 수의 승률, Elo 차이, 시간, peak RAM, 노드/연결 수 출력

현재 모델은 정책망이 아니라 **현재 수를 둔 뒤의 상태가치**를 평가합니다. 각 합법 수를 한 번 적용하고 상대 관점의 값을 부호 반전하여 가장 좋은 수를 고릅니다.

## 빠른 실행

Rust 1.70+만 필요하며 외부 crate나 네트워크가 필요 없습니다.

```bash
mkdir -p data checkpoints
cargo run --release -- generate data/train.csv 1000 42
cargo run --release -- train data/train.csv checkpoints/small.nsn 10 42 128 8 8 2
cargo run --release -- benchmark checkpoints/small.nsn 100 42
```

더 작은 노트북 설정:

```bash
cargo run --release -- generate data/tiny.csv 200 7
cargo run --release -- train data/tiny.csv checkpoints/tiny.nsn 2 7 64 4
cargo run --release -- eval checkpoints/tiny.nsn heuristic 50 7
```

명령:

```text
generate <file> [games=1000] [seed=42]
import-uci <uci.csv> <native.csv>
import-pons <pons.json> <native.csv>
import-zenodo <optimal.csv> <native.csv> [position_limit=100000]
merge <output.csv> <input.csv>...
train <data> <checkpoint> [max_epochs=3] [seed=42] [max_nodes=128] [top_k=8] [max_edges_per_node=8] [patience=2]
eval <checkpoint> [random|heuristic|search] [games=100] [seed=42] [depth=4]
benchmark <checkpoint> [games=100] [seed=42]
inspect <checkpoint>
```

테스트는 `cargo test`, 최적화 빌드 검사는 `cargo test --release`로 실행합니다.

## 데이터 경로

기본 경로는 내장 self-play 생성기입니다. 양쪽 플레이어를 random, heuristic, depth-3 search 조합으로 구성하고 매 수 10% 확률로 무작위 탐험을 합니다. 매 상태는 최종 승자 관점의 `-1/0/+1`로 라벨링됩니다. CSV 한 줄은 `42칸(0 빈칸, 1 X, 2 O),현재 차례,결과,게임 ID`입니다. 게임 ID를 기준으로 90/10 분할하므로 한 게임의 유사 상태가 학습과 검증에 동시에 들어가지 않습니다. 학습 partition만 좌우 반전 증강합니다.

### 공개 데이터

John Tromp가 제공한 UCI Connect-4 opening database는 합법적이고 승리가 강제되지 않은 8-ply 상태 67,557개와 완전게임 결과를 담습니다. 인증 없이 받을 수 있고 현재 가치망의 `-1/0/+1` 목표와 직접 호환됩니다.

```bash
curl -L --fail https://rioultf.users.greyc.fr/uci/files/connect-4/connect-4.data.Z -o data/uci-connect4.data.Z
gzip -dc data/uci-connect4.data.Z > data/uci-connect4.data
cargo run --release -- import-uci data/uci-connect4.data data/uci-connect4.csv
cargo run --release -- train data/uci-connect4.csv checkpoints/uci.nsn 10 42 128 8 8 2
```

Kaggle의 `tbrewer/connect-4`는 CC0이지만 각 행이 종국 보드여서 중간 상태 가치 학습에는 UCI보다 덜 적합합니다. Kaggle 인증/CLI에 빌드가 의존하지 않도록 자동 다운로드 대상에서는 제외했습니다. 2026년 공개 Zenodo 데이터셋에는 100만 개 보드와 Velena 및 3/5/7/9-ply 최적 수가 있습니다.

Zenodo에서는 전체 영상 4.7GB를 받지 않고 `Optimal_Move_Dataset.zip` 47MB만 선택합니다. 다운로드한 파일의 MD5는 Zenodo 메타데이터와 같은 `bd64bbb2db8f189da57f989f792033e1`이어야 합니다.

```bash
curl -L --fail \
  https://zenodo.org/api/records/17578932/files/Optimal_Move_Dataset.zip/content \
  -o data/zenodo-optimal-move.zip
md5sum data/zenodo-optimal-move.zip
unzip data/zenodo-optimal-move.zip -d data/zenodo-optimal

# 저사양 기본: 원본 상태 2만 개 -> 최적/비최적 afterstate 4만 샘플
cargo run --release -- import-zenodo \
  data/zenodo-optimal/Optimal_Move_Dataset.csv data/zenodo-policy-20k.csv 20000
cargo run --release -- merge data/combined.csv \
  data/uci-connect4.csv data/zenodo-policy-20k.csv
```

## 평가 해석

Elo는 대국 score `p=(승+0.5×무)/게임`에서 `400 log10(p/(1-p))`로 계산한 상대 Elo 차이입니다. 0%/100%는 유한 출력을 위해 0.1%/99.9%로 제한합니다. 게임 수가 적으면 분산이 매우 크므로 연구 비교에는 여러 seed와 최소 수천 판을 권장합니다. `VmHWM`은 Linux에서만 제공되며 그 외 환경에서는 0으로 표시됩니다.

## MVP 실측 결과

2026-10-31, 이 저장소의 release 빌드에서 seed 42로 실행했습니다. CPU/호스트에 따라 시간과 RAM은 달라집니다.

```text
generate 1,000 games: 22,274 samples, 25 ms
train 3 epochs: loss 0.891745, 128 nodes / 128 edges, 341 ms, peak RAM 4,200 KiB

100 games each (alternating first player):
vs random:    57-43-0, score .570, Elo +49.0,  28 ms
vs heuristic:  5-95-0, score .050, Elo -511.5, 12 ms
vs search d4:  2-98-0, score .020, Elo -676.1, 107 ms
evaluation peak RAM: 3,140 KiB
```

`cargo test --release` 결과는 단위 테스트 3개와 end-to-end 통합 테스트 1개가 모두 통과했습니다. 이 수치는 구조가 저사양에서 실행 가능함을 보여주지만, 모델의 게임 실력은 아직 random보다 조금 나은 정도이고 강한 기준선에는 크게 뒤집니다. 특히 단일 seed·100판 결과는 성능 결론으로 사용하면 안 됩니다.

## 한계

- 생물학적 뉴런의 근사 모델이 아니라, 가설을 빠르게 반증하기 위한 희소 온라인 학습 장치입니다.
- 생성 조건은 feature coverage와 절대 TD가 아닌 최종결과 오차의 단순 휴리스틱입니다.
- 연결 생성은 현재 활성 입력 하나로 시작하며 숨은 노드 간 연결/재활성화는 아직 없습니다.
- 데이터가 완전탐색 라벨이 아니므로 heuristic self-play 편향이 큽니다.
- 작은 search 기준선은 깊이 제한 평가가 0이라 강한 참조 엔진이 아닙니다.
- RAM은 프로세스 전체 peak이며 모델 단독 메모리가 아닙니다.

다음 연구 단계는 (1) 완전해결 테이블베이스/공개 기보 변환기, (2) n-step TD와 정책 head, (3) 고정형 MLP와 같은 파라미터·연산 예산 비교, (4) 생성/가지치기 ablation, (5) 여러 seed의 신뢰구간입니다.

## 규모 증가 실험

2026-10-31에 seed 42 학습, seed 777 평가로 `1천/5천/2만 판 × 3/10 epoch`를 비교했습니다. 모델 한도는 모두 128노드, top-k 8이며 각 상대와 300판씩 선공을 교대했습니다. W-L-D는 희소 모델 관점입니다.

| 학습 기보 | epoch | 샘플 | 최종 loss | 학습 시간 | random W-L-D (승점률) | heuristic W-L-D (승점률) | search d4 W-L-D (승점률) |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 3 | 22,274 | .891745 | 0.33s | 161-138-1 (53.8%) | 10-290-0 (3.3%) | 16-282-2 (5.7%) |
| 1,000 | 10 | 22,274 | .901211 | 1.08s | 169-131-0 (56.3%) | 14-286-0 (4.7%) | 12-288-0 (4.0%) |
| 5,000 | 3 | 112,699 | .884934 | 1.61s | 178-121-1 (59.5%) | 9-291-0 (3.0%) | 16-283-1 (5.5%) |
| 5,000 | 10 | 112,699 | .883094 | 5.40s | 126-173-1 (42.2%) | 6-294-0 (2.0%) | 12-288-0 (4.0%) |
| 20,000 | 3 | 451,359 | .876248 | 6.71s | 178-121-1 (59.5%) | 9-291-0 (3.0%) | 16-283-1 (5.5%) |
| 20,000 | 10 | 451,359 | .877698 | 22.75s | 178-121-1 (59.5%) | 9-291-0 (3.0%) | 16-283-1 (5.5%) |

이 표는 최초의 **노드당 단일 연결** 구현 결과입니다. 데이터가 1천→5천 판으로 늘어난 3-epoch 모델은 random 상대 승점률이 53.8%→59.5%로 올랐지만, 더 강한 두 상대에는 개선이 없었습니다. 5천→2만 판에서는 선택한 수가 이 평가 세트에서 같아 승패가 완전히 동일했습니다. epoch 증가는 일관된 효과가 없고 5천 판에서는 random 상대 성능을 악화시켰습니다. 모든 모델이 일찍 128노드 한도에 도달해 연결 표현력이 병목인 것으로 판단했습니다.

300판의 승점률 표준오차는 50% 부근에서 약 2.9%p(단순 95% 구간 약 ±5.7%p)이므로 작은 차이는 확정적 개선으로 볼 수 없습니다. 다음 비교에서는 여러 학습 seed의 평균과 신뢰구간이 필요합니다.

## 다중 연결 재실험

노드가 활성화되고 오차가 임계값을 넘으면, 해당 노드에 아직 없는 공동 활성 입력을 연결합니다. 노드당 최대 8연결이며 약한 연결은 기존과 동일하게 가지치기됩니다. 나머지 학습·평가 조건과 seed는 위 단일 연결 실험과 같습니다.

| 학습 기보 | epoch | loss | 노드/연결 | random 승점률 | heuristic 승점률 | search d4 승점률 |
|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 3 | .878751 | 73/332 | 56.5% | 21.0% | 4.2% |
| 1,000 | 10 | .898522 | 73/332 | 56.3% | 0.0% | 4.3% |
| 5,000 | 3 | .868439 | 82/306 | 49.3% | 2.3% | 5.5% |
| 5,000 | 10 | .895524 | 82/306 | 41.0% | 0.0% | 7.0% |
| 20,000 | 3 | .873544 | 118/366 | **63.5%** | **28.3%** | **16.2%** |
| 20,000 | 10 | .873279 | 125/373 | **71.5%** | 1.5% | 15.8% |

가장 균형이 좋은 설정은 **2만 판, 3 epoch**였습니다. 단일 연결의 같은 설정과 비교하면 random은 59.5%→63.5%, heuristic은 3.0%→28.3%, search는 5.5%→16.2%로 상승했습니다. 다중 연결은 더 적은 노드로 306~373개 연결을 유지했고, 노드 한도에 즉시 포화되던 현상도 완화했습니다.

반면 10 epoch에서는 random 성능만 71.5%까지 오르고 heuristic 성능이 1.5%로 붕괴했습니다. 최종 loss도 실제 대국 성능과 일관되게 움직이지 않습니다. 이는 최종 승패 라벨 반복 학습이 일부 상태 분포에 과적합되거나 가치 출력이 포화되는 현상으로 보이며, 현재 결과에서는 3 epoch 조기 종료가 적절합니다.

## 노드당 연결 상한 실험

`2만 판, 3 epoch, seed 42, 128노드, top-k 8`을 고정하고 연결 상한만 바꿨습니다. 평가는 위와 같은 seed 777 및 상대별 300판입니다.

| 최대 연결/노드 | loss | 실제 노드/연결 | random 승점률 | heuristic 승점률 | search d4 승점률 |
|---:|---:|---:|---:|---:|---:|
| 1 | .876248 | 128/128 | 59.5% | 3.0% | 5.5% |
| 2 | **.867437** | 128/244 | 61.2% | 5.7% | 7.8% |
| 4 | .889242 | 99/287 | 49.0% | 1.0% | 6.5% |
| 8 | .873544 | 118/366 | **63.5%** | **28.3%** | **16.2%** |
| 16 | .870465 | 45/376 | 61.2% | 0.3% | 9.8% |
| 32 | .873519 | 28/519 | 59.7% | 0.0% | 5.0% |

8연결이 세 상대 모두에서 가장 좋은 균형을 보였습니다. 16과 32에서는 소수의 노드가 지나치게 많은 특징을 흡수하면서 노드 다양성이 각각 45개와 28개로 줄고, heuristic 일반화가 붕괴했습니다. 반대로 2연결의 loss가 가장 낮지만 대국 성능은 8연결보다 낮아, 학습 loss만으로 구조를 선택해서는 안 됩니다. 현 구현에서는 연결 상한 8을 기본값으로 유지합니다.

## 과적합 완화 및 외부 데이터 결과

UCI 67,557개 상태를 좌우 대칭 canonical position ID 단위로 90/10 분할하고 학습 쪽만 좌우 반전했습니다. 따라서 원본 상태와 그 대칭 상태가 train/validation 양쪽으로 누출되지 않습니다. 최대 10 epoch, patience 2에서 epoch 6이 선택되고 epoch 8에 종료됐습니다.

```text
epoch 1: validation loss .517301
epoch 4: validation loss .499328
epoch 6: validation loss .487801  <- selected
epoch 8: validation loss .517907  <- early stop
model: 92 nodes / 348 edges, train 4.50 s, peak RAM 18,052 KiB
```

상대별 300판 결과는 random 84.7%, heuristic 13.7%, search d4 16.7%였습니다. 기존 self-play `2만 판 × 3 epoch` 모델과 비교하면 random 63.5%→84.7%, search 16.2%→16.7%로 개선됐지만 heuristic은 28.3%→13.7%로 낮아졌습니다. UCI가 8-ply opening 상태에 집중되어 있어 특정 heuristic의 전체 게임 분포를 모두 대변하지 못하기 때문으로 보입니다.

새 혼합 self-play 생성기로 만든 2만 판 모델은 조기 종료 후 random 68.8%, heuristic 3.7%, search 10.7%였습니다. 단순히 상대 종류를 섞는 것만으로는 충분하지 않았으며, 다음 단계는 UCI opening과 heuristic/search 전체 게임 상태를 비율 제어해 함께 샘플링하는 것입니다.

## Zenodo 정책 데이터 실험

Zenodo CSV의 `velena_strong`을 우선 사용하고, 사용할 수 없으면 9/7/5/3-ply 순으로 합법 라벨을 선택합니다. 현재 agent가 수를 둔 뒤 상대 관점 가치를 비교하므로 최적 수 afterstate를 `-1`, 동일 보드의 결정론적으로 선택한 다른 합법 수 afterstate를 `+1`로 변환했습니다. 원본 10만 상태에서는 2,187개를 제외하고 20만 샘플, 2만 상태에서는 421개를 제외하고 4만 샘플을 만들었습니다.

| 학습 데이터 | 선택 epoch | 노드/연결 | random | heuristic | search d4 | peak RAM |
|---|---:|---:|---:|---:|---:|---:|
| UCI만 | 6 | 92/348 | **84.7%** | **13.7%** | **16.7%** | 18 MiB |
| UCI + Zenodo 2만 상태 | 4 | 74/347 | 64.0% | 1.3% | 10.7% | 26 MiB |
| UCI + Zenodo 10만 상태 | 2 | 63/321 | 52.7% | 3.7% | 3.8% | 65 MiB |

Zenodo를 단일 가치 출력에 직접 섞으면 데이터가 많을수록 성능이 하락했습니다. 게임 결과의 절대 가치와 한 상태 안에서의 수 선호는 다른 목표인데 같은 `-1/+1` 축으로 강제한 것이 원인입니다. 따라서 Zenodo 데이터와 변환기는 재현 가능한 실험 입력으로 남기되 가치망 학습에서는 제외합니다. 다시 사용하려면 **7개 수에 대한 별도 policy head와 pairwise ranking loss**로 분리해야 합니다.

## UCI + Pascal Pons 가치 데이터

Zenodo 정책 샘플 대신 `davidquarel/connect4-pons-eval`의 6,705개 exact-solver 상태를 추가했습니다. 각 상태는 합법 열마다 현재 플레이어 관점의 Pascal Pons 점수를 가지며, 부호가 `승/무/패`입니다. 합법 열의 최고 점수 부호를 상태 가치로 변환하므로 UCI와 라벨 의미가 같습니다. ply 2~36을 포함해 UCI의 8-ply 편중도 보완합니다.

```bash
curl -L --fail \
  https://huggingface.co/datasets/davidquarel/connect4-pons-eval/resolve/main/pons_eval_dataset.json \
  -o data/pons-eval.json
cargo run --release -- import-pons data/pons-eval.json data/pons-value.csv
cargo run --release -- merge data/combined-uci-pons.csv \
  data/uci-connect4.csv data/pons-value.csv
cargo run --release -- train data/combined-uci-pons.csv \
  checkpoints/external/uci-pons.nsn 10 42 128 8 8 2
```

총 74,262개 가치 샘플을 학습했고 epoch 2가 선택되어 epoch 4에 조기 종료됐습니다. 모델은 96노드/385연결, 학습 2.67초, peak RAM 19,636 KiB였습니다.

| 모델 (seed 777, 각 300판) | random | heuristic | search d4 |
|---|---:|---:|---:|
| UCI only | **84.7%** | 13.7% | 16.7% |
| UCI + Pons value | 82.7% | **47.2%** | **24.5%** |

새 모델을 평가 seed 5개로 총 1,500판씩 확인한 결과는 random 80.6%(1205-287-8), heuristic 48.8%(726-763-11), search d4 22.8%(329-1146-25)였습니다. random 성능을 약간 양보하는 대신 강한 상대에 대한 일반화가 크게 개선됐으므로 현재 권장 체크포인트를 `uci-pons.nsn`으로 변경합니다.

전체 7×6 strong WDL 테이블도 공개되어 있지만 압축 상태만 약 130GB이므로 저사양 프로젝트 범위에서는 제외했습니다.

## Epoch × 노드 × 연결 상한 탐색

UCI+Pons, seed 42, top-k 8, patience 2를 고정하고 먼저 `max epoch 2/4/8/16 × max nodes 64/128/256 × max edges 4/8/16`의 36개 조합을 탐색했습니다. 이어서 `nodes 32/48/64 × edges 12/16/24`의 9개 조합을 세밀하게 확인했습니다. 1차 평가는 상대별 150판, 최종 후보는 평가 seed 5개로 상대별 총 1,500판을 사용했습니다.

대표적인 1차 결과:

| max epoch | max nodes | max edges | 실제 노드/연결 | random | heuristic | search d4 |
|---:|---:|---:|---:|---:|---:|---:|
| 2 | 64 | 4 | 64/219 | 69.7% | 6.0% | 10.0% |
| 2 | 64 | 8 | 64/349 | 82.0% | 56.7% | 18.7% |
| 2 | 64 | 16 | 62/410 | **81.7%** | **66.7%** | 20.7% |
| 2 | 128 | 8 | 96/385 | 82.7% | 46.7% | **25.0%** |
| 4+ | 64 | 16 | 62/411 | 93.3% | 0.0% | 26.7% |

세부 탐색에서는 32노드/24연결이 search 29.3%를 기록했지만 heuristic은 27.3%에 그쳤고, 48노드 조합도 균형 성능이 낮았습니다. 최대 노드가 128이나 256이어도 실제 생성 노드가 104개 이하라 같은 연결 설정에서는 체크포인트가 같았습니다. max epoch 4 이상 역시 patience 2가 대부분 epoch 3~5에 종료해 중복 결과가 많았습니다.

최종 선택은 `max epoch 2, max nodes 64, max edges 16, top-k 8`입니다. 5개 평가 seed 결과:

| 상대 | W-L-D | 승점률 |
|---|---:|---:|
| random | 1296-203-1 | **86.4%** |
| heuristic | 1103-385-12 | **73.9%** |
| search d4 | 346-1136-18 | **23.7%** |

기존 128노드/8연결 모델의 80.6%/48.8%/22.8%보다 세 상대 모두 개선됐습니다. 연결 16은 epoch 2에서는 가장 좋지만 epoch 3 체크포인트부터 heuristic이 0%로 붕괴하므로, 이 조합에서는 validation loss만 믿고 더 학습하지 않고 epoch 2에서 고정하는 것이 중요합니다. 가장 낮은 validation loss를 보인 모델이 항상 가장 강한 대국 모델은 아니었습니다.

## Search d4 개선 실험

다음 여섯 방향을 구현했습니다. 1차 ablation은 seed 777에서 상대별 150~300판, 기준 모델과 최종 후보는 5개 seed에서 상대별 총 1,500판으로 확인했습니다.

1. Pascal Pons exact 가치 데이터 추가
2. 모델이 search d4에 패한 대국에서 선택이 갈린 상태를 수집하고 exact solver로 라벨링
3. Zenodo 10만 상태를 7열 softmax policy head로 별도 학습
4. 희소 value를 말단 평가로 사용하는 모델 측 depth-2 alpha-beta
5. 열별 즉시 승리/즉시 방어 14개 전술 입력 특징
6. 상위 조합을 5개 평가 seed로 재검증

공식 Pascal Pons solver를 임시 외부 도구로 사용해 500판에서 633개 고유 hard position을 얻었습니다. 이 데이터는 AGPL solver의 출력만 사용하며 solver 코드는 이 프로젝트에 포함하지 않습니다. hard sample을 1회(0.8%)와 10회(약 8%) 혼합해 비교했습니다.

| 방법 | random | heuristic | search d4 | 해석 |
|---|---:|---:|---:|---|
| 기존 tuned value, 1-ply | 86.4% | 73.9% | 23.7% | 기준 |
| 전술 특징 value, 1-ply | 88.3% | 47.3% | 28.3% | search 소폭 개선 |
| hard exact 10× + 전술, 1-ply | 81.7% | 49.2% | 20.7% | 과대표집으로 악화 |
| 전술 value + depth-2 | 99.7% | 41.5% | 72.3% | 가장 큰 단일 구조 효과 |
| 전술 value + policy, depth-1, weight .25 | 100.0% | 66.0% | 64.7% | 탐색 없이도 큰 개선 |
| 전술 value + policy + depth-2, weight .1 | 100.0% | 77.7% | **89.7%** | 최선 조합, seed 777 |
| hard 1× + policy + depth-2 | 99.3% | 94.0% | 68.2% | heuristic 특화, search 하락 |
| hard 10× + policy + depth-2 | 100.0% | 71.0% | 73.5% | 최선보다 낮음 |

최선 조합을 seed 777~781, 상대별 총 1,500판으로 재검증했습니다.

| 상대 | W-L-D | 승점률 |
|---|---:|---:|
| random | 1498-2-0 | **99.9%** |
| heuristic | 1015-196-289 | **77.3%** |
| search d4 | 1262-165-73 | **86.6%** |

가장 큰 효과는 depth-2 탐색이고, policy head가 그 위에 추가 개선을 제공했습니다. 전술 특징은 search 성능을 조금 올렸지만 heuristic에는 손해가 있었습니다. Hard-example mining은 현재의 작은 표본과 단순 재표집 방식에서는 search 일반화를 개선하지 못했으므로 기본 조합에서는 제외합니다. 향후에는 hard 상태마다 단일 WDL만 넣지 말고 열별 exact score를 policy ranking 목표로 사용해야 합니다.

```bash
# 전술 value 모델
cargo run --release -- train data/combined-uci-pons.csv \
  checkpoints/experiments-tactical.nsn 2 42 64 8 16 2

# 별도 policy head
cargo run --release -- train-policy \
  data/zenodo-optimal/Optimal_Move_Dataset.csv \
  checkpoints/external/zenodo-policy.nfp 100000 3 42

# 최선 조합 평가
cargo run --release -- benchmark-hybrid \
  checkpoints/experiments-tactical.nsn \
  checkpoints/external/zenodo-policy.nfp 2 0.1 300 777
```

### Hard 상태의 열별 exact policy 재학습

633개 hard 상태를 Pascal Pons solver의 `-w -a` 모드로 다시 분석해 각 열에 `win/draw/loss/illegal` 값을 부여했습니다. 동률인 최선 수에는 확률을 균등 분배해 policy cross-entropy target으로 사용했습니다.

| hard policy 반복 | policy 샘플 | random | heuristic | search d4 |
|---:|---:|---:|---:|---:|
| 0× (기존 최선) | 100,000 | 100.0% | 77.0% | **88.3%** |
| 1× | 100,633 | 100.0% | 73.7% | 84.0% |
| 5× | 103,165 | 100.0% | 74.3% | 81.3% |
| 10× | 106,330 | 100.0% | 64.0% | 82.3% |

열별 exact 라벨은 단일 WDL 반복보다 원칙적으로 올바르지만, 현재 633개가 특정 모델·상대·seed에서 수집된 편향된 작은 집합이라 기존의 넓은 Zenodo 분포를 교란했습니다. 따라서 기본 policy는 hard 0×를 유지합니다. 다음 hard mining은 여러 모델 체크포인트와 여러 상대·seed에서 수집하고, 일반 데이터와 같은 확률로 중복 없이 사용하는 것이 적절합니다.

## 저사양 및 바둑 모델 자원 비교

NeuroFour 최선 조합을 단일 CPU에서 1,000판씩 세 상대와 평가했습니다.

```text
random:    1000-0-0,    1.136 s
heuristic:  673-138-189, 1.646 s
search d4:  840-119-41,  2.716 s
전체 3,000판: 5.50 s, CPU 99%, peak RAM 3,036 KiB
```

| 항목 | NeuroFour | 공개 바둑 모델의 대표 규모 |
|---|---:|---:|
| trainable 수 | 약 1,238 (value 약 545 + policy 693) | KataGo b6c96 약 103만, b20c256 약 2,348만, 최신 대형 계열 수천만~2억 이상 |
| 체크포인트 | 24 KiB | 네트워크에 따라 수 MB~수백 MB |
| 학습 시간 | value 약 2.5초 + policy 2.13초 | AlphaZero Go 약 13일, KataGo도 다수 GPU에서 장기간 self-play |
| 학습 peak RAM | value 약 19 MiB, policy 약 10.5 MiB | GPU/TPU 메모리와 분산 replay 인프라 필요 |
| 평가 peak RAM | 약 3.0 MiB | 보통 GPU 사용이 유리하며 MCTS tree 메모리가 추가됨 |
| 탐색 | depth-2 alpha-beta | 통상 수백~수천 회 MCTS visits/simulations |
| 보드/실력 | 7×6 Connect Four, search d4 대비 86.6% | 19×19 Go, 프로/초인적 수준 |

AlphaZero 보고값은 self-play 생성에 5,000개 1세대 TPU, 학습에 16개 2세대 TPU, Go 학습 약 13일, 수당 800 MCTS simulations입니다. KataGo 공식 네트워크 문서의 가장 작은 b6c96도 1,027,911 parameters이며, 공개 논문은 30개 미만 GPU로 19일 만에 ELF OpenGo 최종 모델을 넘어섰다고 보고합니다.

이 표는 실력의 직접 비교가 아닙니다. Connect Four와 Go는 상태공간·보드 크기·기준 상대가 다르고, KataGo Elo도 자체 pool 척도입니다. NeuroFour의 장점은 초소형·CPU 재현성·빠른 ablation이고, 바둑 모델의 장점은 훨씬 큰 표현력, MCTS를 통한 정책 개선, self-play 분포 적응, 복잡한 장기 전략입니다. 동일 게임에서의 공정 비교를 하려면 다음 단계로 5×5 또는 9×9 Go 환경을 추가하고 동일 CPU 시간·메모리·visits 조건에서 작은 KataGo/AlphaZero 구현과 대국해야 합니다.

## Go 9×9/19×19 초기 포팅

보드 크기 5~19를 지원하는 Go 규칙 엔진과 저사양 선형 policy/value 기준선을 추가했습니다. 규칙은 사석 제거, 자살수 금지, positional superko, 두 번 pass 종료, komi 6.5의 중국식 영역 점수를 포함합니다. pass는 superko 반복 검사에서 제외하고, 착수 후의 돌 배치만 비교합니다. 학습 데이터는 `size,winner,first_turn,black_setup,white_setup,moves,result,komi` 형식이며 기존 `size,winner,moves` 형식도 읽을 수 있습니다.

```bash
cargo run --release -- go-generate 9 200 data/go/9x9-200.csv 42
cargo run --release -- go-train data/go/9x9-200.csv checkpoints/go/9x9.ngo 3 42
cargo run --release -- go-benchmark checkpoints/go/9x9.ngo 50 777

cargo run --release -- go-generate 19 300 data/go/19x19-300.csv 42
cargo run --release -- go-train data/go/19x19-300.csv checkpoints/go/19x19.ngo 1 42
cargo run --release -- go-benchmark checkpoints/go/19x19.ngo 10 777
```

### SGF 기보 가져오기와 내보내기

[U-Go KGS 기보 아카이브](https://www.u-go.net/gamerecords/)의 2019년 4월 파일은 KGS가 사용을 허가한 7단 이상(또는 양쪽 6단) 대국 1,255판입니다. 주석과 변형 수가 제거되어 있고, 압축 파일에는 SGF 기록이 들어 있습니다.

```bash
mkdir -p data/go/kgs-2019-04
curl -L --fail \
  https://dl.u-go.net/gamerecords/KGS-2019_04-19-1255-.zip \
  -o data/go/kgs-2019-04.zip
unzip -o data/go/kgs-2019-04.zip -d data/go/kgs-2019-04
cargo run --release -- go-import-sgf \
  data/go/kgs-2019-04 data/go/kgs-2019-04.csv
cargo run --release -- go-train \
  data/go/kgs-2019-04.csv checkpoints/go/kgs-2019-04.ngo 3 42 0.1 42
cargo run --release -- go-benchmark \
  checkpoints/go/kgs-2019-04.ngo 10 777
cargo run --release -- go-benchmark \
  checkpoints/go/kgs-2019-04.ngo 10 777 0.5
cargo run --release -- go-export-sgf \
  data/go/kgs-2019-04.csv data/go/kgs-2019-04-export.sgf
```

`go-import-sgf`는 SGF 파일 하나 또는 디렉터리의 `.sgf` 파일을 읽고, 각 컬렉션의 첫 번째 변형을 주계보로 사용합니다. 보드 크기, 승패/무승부, 패스, 핸디캡 배치(`AB`/`AW`/`AE`), 다음 차례(`PL`)를 가져오며 모든 착수를 현재 규칙과 positional superko로 검증합니다. 결과가 없거나 Go 이외 게임인 기록은 건너뜁니다. 현재 아카이브에서는 1,231판을 가져오고 24판을 제외했습니다. `go-export-sgf`는 지원 데이터 형식의 게임을 SGF로 출력하며, `RE` 결과 문자열도 보존합니다.

학습용 CSV 형식은 `size,winner,first_turn,black_setup,white_setup,moves,result,komi`입니다. setup 위치는 보드의 0 기반 인덱스를 `:`로 구분하며 빈 setup은 `-`입니다. 기존 7열 CSV와 `size,winner,moves` 행도 계속 읽을 수 있으며, komi 누락 행은 6.5로 처리합니다. 기록을 적재할 때 winner/result 일치, komi 유효성, 모든 착수의 합법성 및 종료 후 추가 착수를 검증합니다. SGF 왕복 시 승패·착수·핸디캡·차례·결과·komi를 보존합니다.

선형 policy/value 모델은 현재 플레이어 기준 komi를 보드 면적으로 정규화한 입력 feature로 사용합니다. 예전 `NGO1` 체크포인트도 로드할 수 있으며 새 형식 `NGO2`로 저장할 때 komi 가중치는 0으로 초기화되어 기존 출력이 유지됩니다. `go-benchmark`의 선택 인자 `[komi=6.5]`로 기본값 이외의 komi도 지정할 수 있습니다.

`go-train`은 기본적으로 게임 단위 90/10 train/validation 분할을 수행합니다. 분할은 마지막 인자 `split_seed`로 재현되며 같은 기보의 착수들이 두 집합으로 나뉘지 않습니다. 그 앞의 `validation_fraction`은 기본 0.1이며, `0`은 전체 데이터 학습(검증 비활성화), `0 < 비율 < 1`은 검증 활성화입니다. 검증이 켜져 있으면 epoch마다 held-out MSE를 측정하고 가장 좋은 epoch의 체크포인트를 보존합니다. 초기 validation MSE, 선택된 epoch, train/validation MSE, 평가 착수 수를 출력합니다. 검증 MSE는 기력 대국 결과를 대체하지 않습니다.

학습 업데이트도 바로잡았습니다. value 예측은 게임 기록의 komi를 사용하고, `tanh` 출력 미분을 반영한 작은 SGD step을 적용합니다. 그 전에는 기본 komi로 예측하면서 입력 feature는 게임별 komi를 사용했고, 비선형 출력의 gradient도 누락되어 validation MSE가 악화됐습니다. KGS 1,231판에서 split seed 42로 고정한 분할은 학습 1,108판, 검증 123판(23,541착)이었습니다. 3 epoch 한도에서 train seed 42/43/44를 평가하면 세 경우 모두 epoch 1이 선택됐으며, validation MSE는 각각 `.981204`, `.975292`, `.978290`(평균 `.978262`)이었습니다. 초기 baseline `1.000000`보다 평균 약 2.17% 낮지만, 개선 폭은 작고 같은 검증 분할에 한정된 결과입니다. 강도 향상으로 해석할 수 없습니다. 이전 전체 학습의 loss `.141306`은 검증 분할 도입 전 결과라 새 MSE와 직접 비교할 수 없습니다. 기존 `NGO1` 체크포인트 로드 후 벤치마크도 확인했습니다. komi 0.5와 6.5의 각 10판 smoke 대국에서 random 및 heuristic 상대에 모두 0승이었습니다.

합법 수 계산은 빈 칸마다 보드를 복제해 착수 가능 여부를 확인하는 대신, 한 번의 보드 순회로 각 돌 그룹의 활로 수를 계산한 뒤 인접 그룹만 검사합니다. positional superko는 64비트 위치 해시 집합으로 확인하고, 평가용 임시 착수는 history를 복제하지 않습니다. 이 변경 후 같은 seed의 생성 시간은 9×9 200판에서 2.16초→1.29초, 19×19 30판에서 13.56초→6.65초였습니다. 환경에 따라 시간은 달라집니다.

| 보드 | 생성 데이터 | 생성 시간 | 학습 | 모델 파일 | vs random | vs 내장 heuristic | 평가 RAM |
|---|---:|---:|---:|---:|---:|---:|---:|
| 9×9 | 200판 | 1.29초 | 3 epoch, 1.20초 | 145 KiB | 70% (35-15/50) | 58% (29-21/50) | 3.2 MiB |
| 19×19 | 300판 | 70.57초 | 1 epoch, 9.51초 | 2.8 MiB(text) | 20% (2-8/10) | 60% (6-4/10) | 4.9 MiB |

19×19 데이터는 30판에서 300판으로 늘렸지만, 평가 상대별 10판은 여전히 실력 결론에 부족합니다. 특히 내장 heuristic은 포획과 중앙 선호만 사용하는 약한 상대이며 같은 계열의 self-play 데이터도 강한 기력을 보장하지 않습니다. 따라서 이 결과는 개선된 생성 속도와 19×19 규칙→데이터→학습→체크포인트→대국 경로를 확인하는 smoke benchmark입니다.

현재 Go 모델은 완성된 동적 희소 모델이 아니라 공정 비교를 위한 선형 하한선입니다. SGF 입출력과 KGS 공개 기보 학습, 게임 단위 validation 및 best-epoch 보존은 연결했지만, validation 개선은 작고 한 split에서만 확인했습니다. 여러 독립 split과 더 큰 공개 기보에서 재검증한 뒤 ownership·score·활로 보조 head, 지역 연결을 가진 동적 희소 trunk, policy 상위 수를 확장하는 제한 MCTS, GNU Go/Pachi/KataGo 고정 예산 대국 및 최소 수천 판 평가가 남아 있습니다. 현재 KGS smoke 평가만으로 KataGo와 실력을 비교하면 안 됩니다.

#!/usr/bin/env python3
"""Sample KGS game positions and query KataGo analysis for policy/ownership targets."""

import argparse
import csv
import json
import random
import subprocess
import tempfile
from pathlib import Path


def gtp_point(action, size):
    if action == size * size:
        return "pass"
    x, y = action % size, action // size
    column = chr(ord("A") + x + (x >= 8))
    return f"{column}{size - y}"


def action_from_gtp(move, size):
    if move.lower() == "pass":
        return size * size
    column = move[0].upper()
    x = ord(column) - ord("A") - (column > "I")
    y = size - int(move[1:])
    if not (0 <= x < size and 0 <= y < size):
        raise ValueError(f"KataGo returned an invalid point: {move}")
    return y * size + x


class KataGoAnalysis:
    def __init__(self, process, visits):
        self.process = process
        self.visits = visits
        self.next_id = 0

    def analyze(self, size, komi, moves):
        request_id = str(self.next_id)
        self.next_id += 1
        request = {
            "id": request_id,
            "moves": [
                ["B" if ply % 2 == 0 else "W", gtp_point(action, size)]
                for ply, action in enumerate(moves)
            ],
            "rules": "Chinese",
            "komi": komi,
            "boardXSize": size,
            "boardYSize": size,
            "analyzeTurns": [len(moves)],
            "includeOwnership": True,
            "maxVisits": self.visits,
        }
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        while True:
            line = self.process.stdout.readline()
            if not line:
                raise RuntimeError("KataGo analysis process exited before returning a response")
            response = json.loads(line)
            if response.get("id") != request_id or response.get("isDuringSearch", False):
                continue
            if "error" in response:
                raise RuntimeError(f"KataGo analysis failed: {response['error']}")
            move_infos = response.get("moveInfos", [])
            ownership = response.get("ownership")
            if not move_infos or ownership is None or len(ownership) != size * size:
                raise RuntimeError("KataGo response is missing policy or ownership targets")
            policy_counts = {}
            for info in move_infos:
                move_visits = int(info.get("visits", 0))
                if move_visits > 0:
                    action = action_from_gtp(info["move"], size)
                    policy_counts[action] = policy_counts.get(action, 0) + move_visits
            total = sum(policy_counts.values())
            if total <= 0:
                raise RuntimeError("KataGo returned no visited policy moves")
            policy = ",".join(
                f"{action}:{count / total:.8g}"
                for action, count in sorted(policy_counts.items())
            )
            return policy, ",".join(f"{float(value):.7g}" for value in ownership)

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.write(
                '{"id":"shutdown","action":"terminate","terminateId":"shutdown"}\n'
            )
            self.process.stdin.flush()
            self.process.stdin.close()
            try:
                self.process.wait(timeout=30)
            except subprocess.TimeoutExpired as error:
                self.process.terminate()
                self.process.wait()
                raise RuntimeError("KataGo analysis process did not terminate cleanly") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("games_csv", type=Path)
    parser.add_argument("training_tsv", type=Path)
    parser.add_argument("validation_tsv", type=Path)
    parser.add_argument("--katago", required=True, help="KataGo executable")
    parser.add_argument("--model", required=True, help="KataGo neural net model")
    parser.add_argument("--config", required=True, help="KataGo analysis config")
    parser.add_argument("--visits", type=int, default=128)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--games", type=int, default=400, help="maximum eligible games to sample")
    parser.add_argument("--positions-per-game", type=int, default=4)
    parser.add_argument("--validation-fraction", type=float, default=0.2)
    args = parser.parse_args()
    if args.visits <= 0 or args.games < 2 or args.positions_per_game <= 0:
        parser.error("visits and positions-per-game must be positive; games must be at least 2")
    if not 0.0 < args.validation_fraction < 1.0:
        parser.error("validation-fraction must be between zero and one")

    with args.games_csv.open(newline="") as source:
        reader = csv.reader(source)
        games = []
        for row_number, row in enumerate(reader, 1):
            if len(row) < 8:
                raise ValueError(f"expected at least 8 CSV fields at row {row_number}")
            size, first_turn = int(row[0]), int(row[1])
            if size != 19 or first_turn != 1 or row[3] != "-" or row[4] != "-":
                continue
            moves = [int(value) for value in row[5].split()]
            if len(moves) < 40 or any(not 0 <= move <= size * size for move in moves):
                continue
            komi = float(row[7])
            games.append((row_number, size, komi, moves))
    if len(games) < 2:
        raise ValueError("fewer than two eligible 19x19 games with no setup stones")

    rng = random.Random(args.seed)
    rng.shuffle(games)
    games = games[: min(args.games, len(games))]
    validation_count = max(1, round(len(games) * args.validation_fraction))
    validation_games = games[:validation_count]
    training_games = games[validation_count:]
    if not training_games:
        raise ValueError("sampling produced no training games")

    def selected_plies(move_count):
        candidates = sorted(
            {
                max(20, min(move_count - 2, round(move_count * fraction)))
                for fraction in (i / (args.positions_per_game + 1) for i in range(1, args.positions_per_game + 1))
            }
        )
        return candidates[: args.positions_per_game]

    with tempfile.TemporaryDirectory(prefix="neurogo-katago-") as work_dir:
        engine = subprocess.Popen(
            [args.katago, "analysis", "-model", args.model, "-config", args.config,
             "-override-config",
             f"logDir={work_dir},maxVisits={args.visits},numAnalysisThreads=1,"
             "numSearchThreadsPerAnalysisThread=1,nnRandomize=false"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1
        )
        # Reuse the already-started process while keeping request/response matching centralized.
        analyzer = KataGoAnalysis(engine, args.visits)
        counts = {"training": 0, "validation": 0, "ownership_nonzero": 0}
        try:
            for label, selected, output_path in (
                ("training", training_games, args.training_tsv),
                ("validation", validation_games, args.validation_tsv),
            ):
                output_path.parent.mkdir(parents=True, exist_ok=True)
                with output_path.open("w") as output:
                    for _, size, komi, moves in selected:
                        for ply in selected_plies(len(moves)):
                            policy, ownership = analyzer.analyze(size, komi, moves[:ply])
                            history = " ".join(map(str, moves[:ply]))
                            output.write(
                                f"{size}\t{komi}\t{history}\t{policy}\t{ownership}\n"
                            )
                            counts[label] += 1
                            counts["ownership_nonzero"] += sum(
                                float(value) != 0.0 for value in ownership.split(",")
                            )
        finally:
            analyzer.close()
    print(
        f"teacher_data seed={args.seed} games={len(games)} "
        f"training_games={len(training_games)} validation_games={len(validation_games)} "
        f"training_positions={counts['training']} validation_positions={counts['validation']} "
        f"ownership_nonzero_points={counts['ownership_nonzero']} "
        f"visits={args.visits} train={args.training_tsv} validation={args.validation_tsv}"
    )


if __name__ == "__main__":
    main()

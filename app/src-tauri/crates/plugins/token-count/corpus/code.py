import json
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Entry:
    name: str
    size: int

    def kilobytes(self) -> float:
        return round(self.size / 1024, 2)


def load(folder: Path) -> list[Entry]:
    entries = []
    for item in sorted(folder.iterdir()):
        if item.is_file():
            entries.append(Entry(item.name, item.stat().st_size))
    return entries


def main(argv: list[str]) -> int:
    folder = Path(argv[1]) if len(argv) > 1 else Path(".")
    for entry in load(folder):
        print(f"{entry.name:<32} {entry.kilobytes():>10.2f} KB")
    json.dump({"count": len(load(folder))}, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

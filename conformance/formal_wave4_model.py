#!/usr/bin/env python3
from dataclasses import dataclass

@dataclass(frozen=True)
class View:
    checkpoint: int
    mutates: bool

def observe(previous: View, checkpoint: int) -> View:
    assert checkpoint >= previous.checkpoint
    return View(checkpoint, False)

def main() -> None:
    a = View(0,False); b = observe(a,1); c = observe(b,2)
    assert not a.mutates and not b.mutates and not c.mutates
    assert a.checkpoint <= b.checkpoint <= c.checkpoint
    print('read checkpoint view: non-mutating and monotonic')

if __name__ == '__main__': main()

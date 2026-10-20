# Spec

## Protocol

Each device keeps a log of changes. A change has an id, a time and the new text of one note.

## Retry policy

A failed sync waits 2, 4, 8 and then 16 seconds. After that it waits for the next change. The retry policy never
sends more than one request per second.

## Conflicts

Two changes to the same note keep both versions. The person picks one in the app.

## Links

Back to the [plan](plan.md) and the [readme](../README.md). The [missing page](gone.md) is a broken link.

# Diagrams

```mermaid
flowchart LR
  A[Sensor] --> B{Valid?}
  B -->|yes| C[Store]
  B -->|no| D[Flag]
```

```mermaid
sequenceDiagram
  participant B as Buoy
  participant G as Gateway
  B->>G: reading
  G-->>B: ack
```

A Mermaid diagram with a syntax error:

```mermaid
flowchart LR
  A[Open bracket --> B
  this is not mermaid ((
```

```plantuml
@startuml
actor Person
Person -> App : opens a file
App --> Person : shows it
@enduml
```

A PlantUML diagram with a syntax error:

```plantuml
@startuml
this is -> -> not valid ::: plantuml
@enduml
```

```dot
digraph G {
  rankdir=LR;
  read -> check -> publish;
  check -> flag [label="doubtful"];
}
```

A Graphviz diagram with a syntax error:

```dot
digraph G {
  a -> ;
  }}}
```

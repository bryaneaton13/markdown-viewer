# Mermaid diagrams

Fenced `mermaid` blocks are drawn as diagrams in the reader's theme. Hover a diagram for **Copy** and **Code**. A block that can't be drawn stays as code.

## Project lifecycle

```mermaid
flowchart TD
  create[Create the project]
  proposal[Fill in the proposal]
  plan[Build a plan per language]
  approval[Send for approval]
  approved{Approved?}
  progress[Open a progress period]
  report[Fill and submit the period]
  next[Next period opens]
  amend[Amendment: questions plus a revised plan]
  apply[Approved amendment replaces the live plan]
  close[Complete, transition, or archive]

  create --> proposal
  proposal --> plan
  plan --> approval
  approval --> approved
  approved -->|changes requested| proposal
  approved -->|yes| progress
  progress --> report
  report --> next
  next --> report
  next --> amend
  amend --> apply
  apply --> report
  report --> close
```

## How a document is read

A wide left-to-right chart shrinks to fit the page, then scrolls sideways.

```mermaid
flowchart LR
  src[Markdown source] --> parse(Parse)
  parse --> ast[(AST)]
  ast --> math{{Math?}}
  ast --> mer{Mermaid?}
  mer -->|yes| render[Render SVG]
  mer -->|no| code[Code block]
  subgraph Cache
    direction TB
    key[hash + theme] --> img[Image]
  end
  render --> key
  img -.-> view([Reader view])
  code ==> view
```

## Sequence

```mermaid
sequenceDiagram
  participant U as Reader
  participant A as App
  participant W as Watcher
  U->>A: Open project-lifecycle.md
  A->>W: Watch file
  activate W
  Note over A,W: Parse runs off the main thread
  loop Every save
    W-->>A: Changed
    A->>A: Re-parse document
  end
  deactivate W
  A-->>U: Render diagrams
```

## State

```mermaid
stateDiagram-v2
  [*] --> Draft
  Draft --> Review : submit
  Review --> Draft : changes requested
  Review --> Approved : approve
  state Approved {
    [*] --> Active
    Active --> Amending : amend
    Amending --> Active : approved
  }
  Approved --> Archived
  Archived --> [*]
```

## Class

```mermaid
classDiagram
  class Document {
    +PathBuf path
    +String source
    +parse() Result
  }
  class Diagram {
    +u64 hash
    +f32 width
    +f32 height
    +render(theme) Image
  }
  Document "1" *-- "many" Diagram : contains
```

## Entity relationship

```mermaid
erDiagram
  PROJECT ||--o{ LANGUAGE : covers
  LANGUAGE ||--|{ PLAN : has
  PLAN ||--o{ PERIOD : "reported in"
  PROJECT {
    string id PK
    string name
  }
```

## Gantt

```mermaid
gantt
  title Mermaid support
  dateFormat YYYY-MM-DD
  section Build
  Plugin          :done, a1, 2026-11-12, 2d
  Theming         :active, a2, after a1, 2d
  section Ship
  Visual QA       :a3, after a2, 1d
  Release         :milestone, after a3, 0d
```

## Pie

```mermaid
pie title Time spent
  "Reading" : 55
  "Searching" : 25
  "Diagrams" : 20
```

## Mind map

```mermaid
mindmap
  Reader
    Markdown
      Tables
      Math
      Mermaid
    Workspace
      Sidebar
      Search
```

## Git graph

```mermaid
gitGraph
  commit
  branch feature
  checkout feature
  commit
  commit
  checkout main
  merge feature
  commit
```

## Stays as code

A diagram with a syntax error:

```mermaid
flowchart TD
  a[Start] -->|label b
```

A diagram with nothing in it:

```mermaid
sequenceDiagram
  A->>
```

A diagram type the renderer doesn't know:

```mermaid
zenuml
  title Not supported
```

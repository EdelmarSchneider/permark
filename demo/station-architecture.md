---
title: How a reading travels
owner: Tomás Varela
status: stable
updated: 2026-11-12
tags: [architecture, platform, diagrams]
---

# How a reading travels

Every ten minutes, twelve buoys on Larkhaven Sound wake up, measure the water and go back to sleep. This page follows one reading from a sensor one meter down to your screen, a trip of less than a minute.

```mermaid
flowchart TB
  subgraph sea [Sea to shore]
    direction LR
    S[Sensors] --> M[Buoy MCU] --> R[LoRa radio] --> G[3 gateways]
  end
  subgraph shore [Ashore]
    direction LR
    I[Ingest] --> Q[QC engine] --> T[Time-series store] --> A[Open API]
  end
  sea --> shore
  classDef accent fill:#4f46e5,stroke:#4f46e5,color:#eef2ff
  class Q accent
```

From the API, the same numbers reach the harbor dashboard, Gull's weekly bulletin and any researcher who asks. Nothing on this path is exotic: each part is small, boring and replaceable, which is exactly what you want from equipment that spends the winter in salt water.

## The 40-second wake cycle

A buoy spends most of its life asleep. Its <abbr title="microcontroller unit">MCU</abbr> wakes on a timer, powers the sensors, takes a reading, sends it and goes back to sleep. One wake lasts 40 seconds; the other 9 minutes and 20 seconds are quiet.

```plantuml
@startuml
skinparam shadowing false
skinparam roundCorner 10
skinparam ArrowColor #64748b
participant "Buoy MCU" as MCU
participant "Sensors" as SEN
participant "LoRa radio" as RAD
participant "Gateway" as GW
participant "Ingest" as ING

== Wake, t = 0 s ==
MCU -> SEN : power on
SEN --> MCU : ready after 20 s warm-up
MCU -> SEN : sample for 8 s
SEN --> MCU : T, S, pH, DO + wave summary
== Send, t = 28 s ==
MCU -> RAD : 24-byte frame
RAD -> GW : uplink
GW --> RAD : ack
GW -> ING : forward frame
== Sleep, t = 40 s ==
MCU -> SEN : power off
MCU -> MCU : sleep 9 min 20 s
@enduml
```

The gateway answers the radio at once and forwards the frame on its own time, so the radio never waits for the shore. Its listening window stays at three seconds.

## Power during a wake

Where does the energy go? Mostly into warming up the sensors. The radio, despite its reputation, is on for barely four seconds.

```plantuml
@startuml
skinparam shadowing false
skinparam roundCorner 10
scale 1 as 13 pixels
robust "Buoy MCU" as MCU
robust "Sensors" as SEN
robust "LoRa radio" as RAD
concise "Battery draw" as BAT

@-4
MCU is Asleep
SEN is Off
RAD is Off
BAT is "0.5 mW"

@0
MCU is Awake
SEN is Warming
BAT is "0.65 W"

@20
SEN is Sampling

@28
SEN is Off
RAD is Sending
BAT is "0.2 W"

@29
RAD is Listening

@32
RAD is Off
BAT is "0.05 W"

@40
MCU is Asleep
BAT is "0.5 mW"

@46
MCU is Asleep
@enduml
```

Add it up and one wake costs about 19 joules. With the wave sensor ticking along between wakes, a buoy uses under 1 Wh a day: the 10 W panels earn that back in a few minutes of full sun, and the 154 Wh battery could carry a buoy through five months of gray skies.

## Inside a buoy

Each buoy is a float, a short tower and a sensor cage, held in place by a chain. The electronics bay sits inside the float, just above the waterline.

![Cutaway of a Tidewell buoy: LoRa antenna and marker light on top, solar panels on the tower, MCU and battery in the float, sensor cage one meter down, mooring chain to a seabed weight](assets/buoy-anatomy.svg)

## Twelve buoys, three gateways

The buoys talk to three gateways on shore, four buoys each. Every buoy also knows a fallback gateway; the dashed line shows Gannet Point's.

```dot
digraph network {
  rankdir=TB;
  nodesep=0.15;
  ranksep=0.5;
  node [shape=box, style="rounded,filled", fontsize=12, width=0.6, height=0.34];
  edge [arrowsize=0.6];

  subgraph cluster_north {
    label="North Light"; style="rounded,dashed";
    "LH-01"; "LH-02"; "LH-03"; "LH-04";
    north [label="gateway"];
  }
  subgraph cluster_avery {
    label="Avery Pier"; style="rounded,dashed";
    "LH-05"; "LH-06"; "LH-07"; "LH-08";
    avery [label="gateway"];
  }
  subgraph cluster_saddle {
    label="Saddle Hill"; style="rounded,dashed";
    "LH-09"; "LH-10"; "LH-11"; "LH-12";
    saddle [label="gateway"];
  }

  ingest [label="Ingest, Port Avery"];

  {"LH-01" "LH-02" "LH-03" "LH-04"} -> north;
  {"LH-05" "LH-06" "LH-07" "LH-08"} -> avery;
  {"LH-09" "LH-10" "LH-11" "LH-12"} -> saddle;
  "LH-04" -> avery [style=dashed];
  north -> ingest;
  avery -> ingest;
  saddle -> ingest;
}
```

When a gateway loses its link to shore, it buffers frames and replays them later. The [ingest log for November 12](station/ingest-2026-11-12.log) shows Saddle Hill doing exactly that, with no reading lost.

## A buoy's life

A buoy is always in one of a few states. Most of the season it simply alternates between sampling and sleeping.

```mermaid
stateDiagram-v2
  [*] --> Docked
  Docked --> Calibrating : season prep
  Calibrating --> Deployed : bath test passed
  state Deployed {
    direction LR
    Sleeping --> Sampling : every 10 min
    Sampling --> Sleeping : frame sent
    Sampling --> Fault : sensor error
    Fault --> Sampling : self-test passed
  }
  Deployed --> Recovered : end of season
  Recovered --> Docked : cleaned and logged
```

## The data model

Behind the API sit five tables. A station hosts one deployment for every buoy moored there; each deployment carries its sensors and records its readings; a reading may collect QC flags.

```mermaid
erDiagram
  direction LR
  STATION ||--o{ DEPLOYMENT : ""
  DEPLOYMENT ||--|{ SENSOR : ""
  DEPLOYMENT ||--o{ READING : ""
  READING ||--o{ QC_FLAG : ""
  STATION {
    text id PK
    text name
    text gateway
  }
  DEPLOYMENT {
    int id PK
    text station FK
    date deployed
    text firmware
  }
  SENSOR {
    text serial PK
    text kind
    date calibrated
  }
  READING {
    int id PK
    time taken_at
    float temp_c
    float ph
  }
  QC_FLAG {
    int reading FK
    text rule
    text decision
  }
```

Readings belong to a deployment, not to a station. A buoy swapped mid-season never mixes its numbers with its replacement's.

## Configuration

Two plain files steer the whole pipeline. The buoy reads [its settings](station/buoy.toml) at every boot, and the ingest service checks each reading against [the QC rules](station/qc-rules.yaml). Here are the first lines of the rules:

```yaml title="station/qc-rules.yaml" {4-6}
# Tidewell QC rules, applied by the ingest service to every reading.
# Units follow each sensor: °C, PSU, pH units, mg/L and meters.
version: 3
order: [range, spike, flatline, drift]
reject_on: [range]
publish_flagged: true

# Physical limits as [min, max]. Outside them, a reading is rejected.
range:
  temp_c: [-2.0, 35.0]
  sal_psu: [0.0, 42.0]
  ph: [7.2, 8.6]
  do_mgl: [0.0, 15.0]
  hs_m: [0.0, 12.0]
```

Lines 4 to 6 are the whole policy: the order of the checks, the one failure that rejects a reading outright, and the promise that flagged readings are still published, flag attached. When you want to point a colleague at a rule, a reference like `station/qc-rules.yaml:5` opens the file at that line. The early warning about Kittiwake Shoal's pH sensor is at `station/ingest-2026-11-12.log:22`.

## Decision: LoRa, not cellular

We chose the radio before we chose anything else, because it decides the battery, the budget and where buoys can go.

<details>
<summary>ADR-007 · Use LoRa with our own gateways (accepted, June 2025)</summary>

**Context.** Buoys send 24 bytes every 10 minutes from up to 14 km offshore. The inner Sound, around Heron Flats, has patchy cell coverage.

| Option | Range over water | Energy per send | Cost per buoy, 5 years | Verdict |
|---|---|---:|---:|---|
| <abbr title="Long Range, a low-power radio protocol">LoRa</abbr>, 3 own gateways | 10–15 km | 0.4 J | $263 | Chosen |
| LTE-M cellular | patchy past Saddle Hill | 4 J | $345 | Fallback study |
| Satellite short-burst | anywhere | 20 J | $10,760 | Too costly |

**Decision.** LoRa in the US915 band, spreading factor 9, three gateways on land we can reach by car.

**Consequences.** No monthly fees and no gaps, but we maintain three gateways ourselves. Frames must stay small: at spreading factor 9, a frame has to fit in the band's 400 ms limit on air.

</details>

---

Next: [Tidewell Open Data API](tidewell-api.md) · [Start here](README.md)

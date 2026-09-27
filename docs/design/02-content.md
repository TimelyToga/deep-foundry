# Deep Foundry: content

This file lists the materials, reactions, parts, buildings, recipes, production chains, research and upgrades.

All numbers are first guesses for balance. When the data files exist (`assets/data/`), the data files are the source of truth. Update this file when a big change happens.

Tier 0 and Tier 1 have full detail, because the first playable version uses them. Later tiers have less detail.

## 1. Units and base numbers

| Item | Value |
|---|---|
| Simulation speed | 60 ticks per second |
| 1 cell | 1 unit of material |
| Ingot | 16 units |
| Plate | 16 units |
| Rod | 8 units |
| Gear | 48 units |
| Pipe section (part) | 48 units |
| Wire | 4 units |
| Bolt | 2 units |
| Wood belt speed | 8 cells per second |
| Iron belt speed | 16 cells per second |
| Steel belt speed | 32 cells per second |
| Alloy belt speed | 64 cells per second |
| Bronze pipe flow | 100 units per second |
| Iron pipe flow | 150 units per second |
| Steel pipe flow | 300 units per second |

All methods that make a part use the same amount of material. Later methods are faster and automatic. Casting in a mold is free of power but slow, because the metal must cool.

Phase changes use a small gap to stop cells from changing back and forth. Example: copper melts at 1085 °C and freezes at 1075 °C.

## 2. Materials

### 2.1 Terrain and rock

| Id | Name | Phase | Density | Hardness | Broken form | Heat rules | Notes |
|---|---|---|---|---|---|---|---|
| air | Air | empty | 1.2 | – | – | moves toward the layer temperature | Fire needs air next to it. |
| dirt | Dirt | powder, small | 1300 | soft | – | – | With water: mud. |
| grass | Grass | solid | 1300 | soft | dirt | burns at 250 °C | Spreads on dirt in light. |
| sand | Sand | powder, small | 1600 | soft | – | melts at 1700 °C into molten glass | |
| gravel | Gravel | powder, large | 1800 | soft | – | – | |
| clay | Clay | powder, small, steep piles | 1700 | soft | – | at 900 °C: terracotta | |
| mud | Mud | liquid, very slow | 1600 | – | – | at 60 °C: dirt | Dries slowly in air. |
| stone | Stone | solid | 2600 | stone | gravel | melts at 1200 °C into lava | |
| limestone | Limestone | solid | 2700 | stone | crushed limestone | at 900 °C: quicklime + CO₂ | Flux for iron smelting. |
| granite | Granite | solid | 2750 | hard | gravel | melts at 1250 °C into lava | |
| basalt | Basalt | solid | 3000 | very hard | crushed basalt | melts at 1200 °C into lava | |
| obsidian | Obsidian | solid | 2400 | very hard | obsidian shards | melts at 1200 °C into lava | Forms when lava meets water. |
| bedrock | Bedrock | solid | – | cannot break | – | – | World edges. |
| ice | Ice | solid | 917 | soft | snow | melts at 0 °C | Floats on water. |
| snow | Snow | powder, small | 300 | soft | – | melts at 0 °C | |
| frozen_ground | Frozen ground | solid | 1500 | stone | dirt | above 0 °C: mud | Tundra. |
| peat | Peat | powder | 700 | soft | – | burns at 250 °C, much smoke | Swamp fuel. |
| wood | Wood | solid | 600 | soft | wood chips | burns at 300 °C; with no air: charcoal | Floats on water. |
| leaves | Leaves | solid | 400 | soft | – | burns at 200 °C | |
| rubber_wood | Rubber tree wood | solid | 600 | soft | wood chips | same as wood | The extractor gets resin from it. |
| crystal_rock | Crystal rock | solid | 3200 | extreme | crystal gravel | melts at 2000 °C | Crystal Deep layer. |
| core_rock | Core rock | solid | 6000 | extreme | core gravel | melts at 3000 °C | Core layer. |

### 2.2 Ores

An ore vein is a solid. When the player digs it, it becomes the raw ore powder.

| Ore | Found in | Hardness | Density | Metal | Byproducts | First method |
|---|---|---|---|---|---|---|
| Malachite | surface, temperate | soft | 4000 | copper | – | with charcoal at 1100 °C |
| Cassiterite | gravel in rivers and lakes | soft | 7000 | tin | iron | with charcoal at 900 °C |
| Coal | surface outcrops, seams in upper stone | soft / stone | 1350 | fuel | sulfur (as SO₂ when it burns) | burns at 450 °C |
| Magnetite | upper stone | stone | 5200 | iron | – (magnetic) | with coke at 1400 °C |
| Hematite | upper stone | stone | 5300 | iron | – | with coke at 1400 °C |
| Chalcopyrite | upper stone | stone | 4200 | copper | iron, sulfur, gold (trace) | roast, then smelt |
| Galena | deep rock | hard | 7500 | lead | silver, sulfur | roast, then with coke at 1000 °C |
| Sphalerite | deep rock | hard | 4000 | zinc | cadmium, gallium, sulfur | roast; zinc boils at 907 °C, so collect the vapor |
| Gold quartz | deep rock | hard | 3000 | gold | silver | crush, then wash or use mercury |
| Native sulfur | deep rock, volcanic | stone | 2070 | sulfur | – | melts at 115 °C |
| Halite | desert, deep rock | stone | 2160 | salt | – | dissolves in water |
| Bauxite | deep rock | stone | 2500 | aluminium | iron (red mud), gallium | Bayer process, then aluminium cell |
| Quartz | deep rock | hard | 2650 | silicon | – | with carbon at 1900 °C (arc furnace) |
| Wolframite | magma | very hard | 7300 | tungsten | manganese, iron | chemical route (Tier 4) |
| Chromite | magma | very hard | 4700 | chromium | iron | electric blast furnace |
| Pentlandite | magma | very hard | 4800 | nickel | iron, cobalt, sulfur | roast, then smelt |
| Ilmenite | magma | very hard | 4700 | titanium | iron | chlorine route (Tier 4) |
| Kimberlite | magma (vertical pipes) | very hard | 2900 | diamond | – | crush and sort |
| Uraninite | crystal deep | extreme | 10000 | uranium | radium, lead | acid leach (radioactive) |
| Monazite | crystal deep | extreme | 5100 | rare earths | thorium | acid route (Tier 5) |

### 2.3 Processed powders

| Name | Density | Grain | Rules | Made by |
|---|---|---|---|---|
| Crushed ore (one for each ore) | as ore | medium | – | crusher, stamp mill |
| Washed ore (one for each ore) | as ore | medium | – | sluice, washer |
| Metal dust (one for each metal) | as metal | small | smelts fast; iron and aluminium dust can burn | crusher, centrifuge |
| Wood chips | 300 | medium | burns at 250 °C, fast | cutting wood |
| Charcoal | 400 | medium | burns at 350 °C, hot, little smoke | wood heated with no air |
| Coke | 600 | medium | burns at 500 °C; very hot with an air blast | coke oven |
| Ash | 600 | small | glass flux | burning |
| Crushed limestone | 2700 | medium | at 900 °C: quicklime + CO₂ | crusher |
| Quicklime | 1500 | small | with water: slaked lime and much heat | heated limestone |
| Cement | 1500 | small | with water: wet concrete | mixer: quicklime + crushed slag |
| Salt | 2160 | small | dissolves in water; melts at 801 °C | brine evaporation, halite |
| Sulfur dust | 2070 | small | melts at 115 °C; burns at 230 °C into SO₂ | crushed sulfur |
| Crushed slag | 2800 | medium | – | crushed slag blocks |
| Rust | 5000 | small | – | iron in water and air, slowly |
| Gunpowder | 1700 | small | explodes with fire | mixer: charcoal + sulfur + saltpeter (later) |
| Thermite | 4000 | small | burns at 2500 °C | mixer: aluminium dust + rust |
| Silicon | 2330 | small | – | arc furnace |
| Soda ash | 2500 | small | glass flux | chemical route (Tier 2) |
| Red mud | 2000 | small | waste; toxic | Bayer process |
| Graphite | 2200 | small | – | from coke at high heat |
| Float stone dust | –500 | small | falls up | crushed float stone |
| Nanites | 3000 | small | eat metal (section 10) | nanite assembler |

### 2.4 Liquids

| Name | Density | Flow | Rules | Notes |
|---|---|---|---|---|
| Water | 1000 | fast | freezes at 0 °C; boils at 100 °C | Conducts electricity (for short circuits). |
| Brine (salt water) | 1200 | fast | boils at 105 °C into steam + salt; freezes at −20 °C | |
| Dirty water | 1050 | fast | carries small dust; settles slowly in a tank | Washer waste. The centrifuge gets the dust out. |
| Mud | 1600 | very slow | at 60 °C: dirt | |
| Crude oil | 870 | slow | burns at 250 °C; above 350 °C it becomes oil vapors | |
| Heavy oil | 950 | slow | burns at 300 °C | |
| Naphtha | 750 | fast | burns easily; evaporates | |
| Fuel | 830 | medium | burns at 250 °C | Combustion generator. |
| Lubricant | 900 | medium | – | Machine upgrade. |
| Creosote | 1080 | medium | burns at 400 °C | Treated wood, fuel. |
| Tar | 1200 | very slow | burns | |
| Resin | 1100 | slow | at 150 °C: rubber | From rubber tree wood. |
| Sulfuric acid | 1830 | medium | corrodes metals; boils at 337 °C | |
| Hydrochloric acid | 1180 | fast | corrodes metals; releases HCl gas | |
| Sodium hydroxide solution | 1500 | medium | corrodes aluminium | |
| Iron sulfate solution | 1200 | fast | – | Acid on iron. |
| Copper sulfate solution | 1200 | fast | – | Blue. Electrolytic refining. |
| Mercury | 13500 | fast | boils at 357 °C (toxic vapor); freezes at −39 °C | Very dense. |
| Liquid nitrogen | 808 | fast | boils at −196 °C | Freezes what it touches. |
| Liquid oxygen | 1141 | fast | boils at −183 °C | Makes fire much hotter. |
| Lava | 2600 | slow | below 800 °C: basalt; with water: obsidian | Glows. |
| Wet concrete | 2400 | slow | after 30 s: concrete | |
| Molten glass | 2400 | slow | below 1000 °C: glass | |
| Molten slag | 2800 | slow | below 1200 °C: slag | Floats on molten iron. |
| Molten salt | 1550 | medium | below 790 °C: salt | |
| Molten cryolite | 2100 | medium | below 1000 °C: cryolite | Aluminium cell. |
| Molten sulfur | 1800 | medium | burns | |
| Corium | 8000 | slow | melts most materials; radioactive | Reactor meltdown. |
| Core matter | 20000 | slow | about 8000 °C | Final resource. |

### 2.5 Molten metals

Each metal also has a solid block form and a dust form. Freezing is 10 °C below melting.

| Metal | Melts at | Liquid density | Notes |
|---|---|---|---|
| Tin | 232 °C | 7000 | The first metal. A campfire-heated crucible melts it. |
| Lead | 327 °C | 10600 | Radiation shield. |
| Zinc | 420 °C | 6600 | Boils at 907 °C. |
| Aluminium | 660 °C | 2375 | Light. Floats on most molten metals. |
| Bronze | 950 °C | 8700 | 3 copper + 1 tin. |
| Silver | 962 °C | 9300 | |
| Gold | 1064 °C | 17300 | |
| Copper | 1085 °C | 8000 | |
| Pig iron | 1150 °C | 7000 | High carbon. Brittle. Melts lower than steel. |
| Steel | 1450 °C | 7000 | |
| Stainless steel | 1450 °C | 7500 | |
| Nickel | 1455 °C | 7800 | |
| Titanium | 1668 °C | 4100 | |
| Tungsten | 3422 °C | 17600 | |

### 2.6 Gases

Air has a density of 1.2. Gases lighter than air rise. Gases heavier than air sink.

| Gas | Density | Rules | Danger or use |
|---|---|---|---|
| Steam | 0.6 | below 100 °C: water | Hot steam damages the robot. Power. |
| Smoke | 0.9 | fades after about 5 s | Blocks light and solar power. |
| Methane | 0.7 | – | Explodes with fire or a spark. |
| Coal gas | 0.6 | – | Burns. Boiler fuel. |
| Hydrogen | 0.09 | rises fast | Explodes. Fuel, chemistry. |
| Oxygen | 1.43 | – | Makes fire hotter. |
| Nitrogen | 1.25 | – | Does not react. |
| Carbon dioxide | 1.98 | sinks | Puts out fire. |
| Chlorine | 3.2 | sinks | Toxic, corrosive. Chemistry. |
| Sulfur dioxide | 2.6 | sinks | Toxic. With water: acid water. Causes acid rain. |
| Hydrogen sulfide | 1.36 | sinks | Toxic. Burns into SO₂. |
| Hydrogen chloride | 1.49 | sinks | With water: hydrochloric acid. |
| Mercury vapor | 6.9 | sinks | Toxic. Below 357 °C: mercury. |
| Zinc vapor | 3.0 | – | Below 907 °C: molten zinc. Burns in air into white zinc oxide smoke. |
| Oil vapors (4 kinds) | 2–5 | rise when hot | Each condenses at its own temperature (distillation tower). |
| Ethylene | 1.18 | – | Plastics. |
| Argon | 1.78 | sinks | Does not react. |
| Radon | 9.7 | sinks | Radioactive. |

### 2.7 Fire and special materials

| Name | Rules |
|---|---|
| Fire | Lives 0.3 to 1 s. Rises. Its temperature depends on the fuel. Heats and ignites neighbors. |
| Ember | A burning powder cell (charcoal, coal). Glows. Lives longer than fire. |
| Spark | Moves through conductive cells. Lives a very short time. Ignites flammable gas. |
| Plasma | About 10000 °C. Fusion reactor. |
| Float stone | A solid in veins; its dust falls up. |
| Charged crystal | A solid. Stores charge. Releases sparks when hit or heated. |
| Glow fungus | Grows on wood in dark, wet places. Gives light. |

### 2.8 Building blocks

These are the solid materials that walls and building bodies are made of.

| Block | Made from | Limit | Hardness | Notes |
|---|---|---|---|---|
| Wood block | wood | burns at 300 °C | soft | |
| Treated wood | wood + creosote | burns at 600 °C | soft | Resists fire. |
| Clay brick | fired raw clay brick | 1200 °C | stone | Kiln walls. |
| Terracotta | fired clay cells | 1200 °C | stone | |
| Firebrick | fired raw firebrick | 1800 °C | stone | Low heat conduction. Furnace walls. |
| Glass | molten glass | 1000 °C | stone | Transparent. Acid-proof. Does not conduct. |
| Concrete | wet concrete | 1200 °C | hard | |
| Reinforced concrete | concrete + steel rods | 1400 °C | very hard | Reactor walls. |
| Metal block (each metal) | the metal | its melting point | stone to very hard | Conducts heat. |
| Rubber | resin at 150 °C | burns at 300 °C | soft | Electric insulator. |
| Plastic (PVC) | chemistry (Tier 3) | 150 °C | soft | Acid-proof. |
| Carbon block | graphite | 3000 °C | hard | Aluminium cell lining. |
| Insulation | glass fiber | 1000 °C | soft | Very low heat conduction. |
| Lead block | lead | 327 °C | stone | Radiation shield. |
| Heat-proof casing | tungsten steel (Tier 4) | 3000 °C | very hard | |

## 3. Reactions

"Tag" means any material with that tag. "Air" means an empty cell next to the input.

| # | A | B | Condition | Result |
|---|---|---|---|---|
| 1 | water | – | ≥ 100 °C | steam |
| 2 | steam | – | < 100 °C | water |
| 3 | water | – | < 0 °C | ice |
| 4 | ice | – | > 0 °C | water (cools its neighbors) |
| 5 | water | lava | – | steam + obsidian |
| 6 | lava | – | < 800 °C | basalt |
| 7 | tag: molten metal | water | – | steam burst (small explosion) + the metal freezes |
| 8 | fire | water | – | steam; the fire goes out |
| 9 | snow | – | > 0 °C | water |
| 10 | salt | water | – | brine (the salt dissolves) |
| 11 | salt | ice | – | brine (the ice melts) |
| 12 | brine | – | ≥ 105 °C | steam; sometimes salt stays |
| 13 | dirt | water | – | mud |
| 14 | mud | – | ≥ 60 °C, or slowly in air | dirt |
| 15 | wood | air | ≥ 300 °C | fire; the wood becomes ash and smoke |
| 16 | wood | – | ≥ 300 °C, no air around it for 10 s | charcoal |
| 17 | coal | air | ≥ 450 °C | long fire; ash, smoke, CO₂ and some SO₂ |
| 18 | charcoal | air | ≥ 350 °C | hot fire (1100 °C with bellows); ash, CO₂ |
| 19 | coke | air | ≥ 500 °C | very hot fire (1600 °C with a hot air blast); CO₂ |
| 20 | tag: oil | air | ≥ its flash point | fire and thick smoke |
| 21 | methane, coal gas, hydrogen | fire or spark | – | explosion (the size depends on the amount of gas) |
| 22 | fire | carbon dioxide | – | the fire goes out |
| 23 | fire | oxygen | – | the fire gets 50% hotter |
| 24 | sulfur | air | ≥ 230 °C | blue fire; SO₂ |
| 25 | hydrogen sulfide | air | ≥ 260 °C | fire; SO₂ + steam |
| 26 | gunpowder | fire | – | explosion |
| 27 | thermite | fire | ≥ 1000 °C | burns at 2500 °C; molten iron + aluminium oxide dust |
| 28 | cassiterite (raw, crushed or washed) | charcoal or coke | ≥ 900 °C | molten tin + CO₂ (+ slag from raw ore) |
| 29 | malachite | charcoal or coke | ≥ 1100 °C | molten copper + CO₂ |
| 30 | magnetite or hematite | coke | ≥ 1400 °C | molten pig iron + CO₂ (+ slag if there is no flux) |
| 31 | tag: gangue (ore waste) | limestone or quicklime | ≥ 1200 °C | molten slag |
| 32 | chalcopyrite | air | ≥ 600 °C | roasted chalcopyrite + SO₂ |
| 33 | roasted chalcopyrite | charcoal or coke | ≥ 1200 °C | molten copper + iron dust + CO₂ |
| 34 | galena | air | ≥ 600 °C | roasted galena + SO₂ |
| 35 | roasted galena | coke | ≥ 1000 °C | molten silver-bearing lead + CO₂ |
| 36 | molten zinc | – | ≥ 907 °C | zinc vapor |
| 37 | zinc vapor | air | – | zinc oxide smoke |
| 38 | sand | – | ≥ 1700 °C | molten glass |
| 39 | sand | ash | ≥ 1100 °C | molten glass (the ash is used up) |
| 40 | sand | soda ash | ≥ 1000 °C | molten glass (the soda ash is used up) |
| 41 | molten glass | – | < 1000 °C | glass |
| 42 | clay | – | ≥ 900 °C | terracotta |
| 43 | limestone (solid or crushed) | – | ≥ 900 °C | quicklime + CO₂ |
| 44 | quicklime | water | – | slaked lime + much heat |
| 45 | cement | water | – | wet concrete; after 30 s concrete |
| 46 | sulfuric acid | iron, steel, zinc or tin | – | metal sulfate solution + hydrogen (the metal dissolves) |
| 47 | sulfuric acid | limestone | – | gypsum + CO₂ (bubbles) |
| 48 | tag: acid | sodium hydroxide solution | – | brine + heat |
| 49 | sulfur dioxide | water | – | acid water (a weak acid; it corrodes slowly) |
| 50 | iron or steel | water, with air next to it | slow | rust |
| 51 | mercury | gold dust | – | gold amalgam |
| 52 | gold amalgam | – | ≥ 357 °C | gold dust + mercury vapor |
| 53 | mercury vapor | – | < 357 °C | mercury |
| 54 | chlorine | hydrogen | spark or light | hydrogen chloride + heat |
| 55 | hydrogen chloride | water | – | hydrochloric acid |
| 56 | liquid nitrogen | – | > −196 °C | nitrogen |
| 57 | liquid nitrogen | water | – | ice + nitrogen |
| 58 | resin | – | ≥ 150 °C | rubber |
| 59 | spark | tag: conductive | – | the spark moves into the conductive cell |
| 60 | tag: bare cable (back layer) | water | – | sparks; power loss |
| 61 | nanites | tag: metal | – | nanites + nanites |
| 62 | nanites | – | ≥ 1500 °C | ash |
| 63 | uraninite | – | always | releases heat; sometimes radon |
| 64 | grass | dirt | light, air | grass spreads |
| 65 | glow fungus | wood | dark, wet | the fungus grows |
| 66 | corium | any material that is not heat-proof | – | melts it |

Rule 7 is important for safety: molten metal and water must not meet.

## 4. Parts

| Part | Material | Made by (first way) | Used for |
|---|---|---|---|
| Raw clay brick | 16 clay | hand | kiln → clay brick |
| Clay brick | – | kiln (≥ 900 °C, 20 s) | kiln walls, crucibles, molds, research |
| Raw firebrick | 12 clay + 4 sand | hand, later mixer | kiln → firebrick |
| Firebrick | – | kiln with bellows (≥ 1200 °C) | furnace walls |
| Ingot (each metal) | 16 molten | ingot mold, furnace | most recipes |
| Plate (each metal) | 16 | plate mold, hammer | most recipes |
| Rod (each metal) | 8 | rod mold, lathe | wires, bolts, frames |
| Gear (each metal) | 48 | gear mold, assembler | machines |
| Pipe section (each metal) | 48 | pipe mold, bender | pipes, boilers |
| Wire (copper, tin) | 4 | wire drawer: 1 rod → 2 wires | cables, circuits |
| Insulated wire | 1 wire + 4 rubber | assembler | LV cables |
| Bolt | 2 | lathe | machines |
| Glass pane | 16 glass | pane mold | labs, windows, solar |
| Glass vial | 8 glass | vial mold | research kits |
| Glass tube | 8 glass | tube mold | vacuum tubes |
| Rubber sheet | 16 rubber | press | insulation, belts |
| Circuit board | wood chips + resin | press | circuits |
| Vacuum tube | glass tube + copper wire + steel bolt | assembler | basic circuit |
| Basic circuit | board + 2 vacuum tubes + 4 copper wires | circuit assembler (Tier 2) | LV machines |
| Battery | 2 lead plates + sulfuric acid + rubber case | assembler (Tier 2) | power storage, Electric kit |
| Transistor, good circuit | silicon, plastic, gold wire | Tier 3 | MV and HV machines |
| Microchip | engraved silicon wafer | Tier 4 | HV and EV machines |
| Crystal processor | charged crystal + microchips | Tier 5 | EV machines |
| Research kits | see section 8 | | labs |

## 5. Buildings

### 5.1 Tier 0: Hand and Fire

| Building | Size (tiles) | Cost | What it does |
|---|---|---|---|
| Workbench | 2×2 | 20 wood | Hand crafting is 2× faster near it. |
| Campfire | 1×1 | 10 wood | Burns fuel from its small hopper. Heats what is above it. |
| Wood block, wood slope | 1×1 | 8 wood | Walls and slopes. |
| Ladder | 1×1 | 2 wood | Climbing. |
| Clay brick wall | 1×1 | 2 clay bricks | Kiln walls. |
| Kiln controller | 1×1 | 8 clay bricks + 2 wood | Controls a kiln room. |
| Kiln hatch, kiln tap, kiln door, chimney | 1×1 | clay bricks + wood or tin | Kiln ports: input, liquid output, part access, exhaust. |
| Bellows | 1×1 | 10 wood + 2 tin plates | Water-driven: water must flow through its wheel. Raises the fire temperature in its room by up to 300 °C. |
| Crucible | 1×1 | 4 clay bricks | Holds 200 units of molten metal. Takes heat from the cells below it. Makes alloys. Has a side tap. |
| Molds (ingot, plate, rod, gear, pipe) | 1×1 | 4 clay bricks | Molten material that fills the mold becomes a part when it cools. The mold gets hot. The glass molds (pane, vial, tube) come with the Glass technology in Tier 1. |
| Stamp mill | 2×3 | 30 wood + 4 bronze gears | Water-driven crusher. Water must flow through its wheel. |
| Sluice | 3×1 | 20 wood | Water flows through it. Heavy powder stays in it. Light powder washes out. |
| Hopper | 1×1 | 6 wood | Collects powder. Releases it at a set rate. Filter. |
| Wood chute | 1×1 | 4 wood | Sloped channel. |
| Wood belt | 1×1 | 4 wood | 8 cells per second. Can burn. |
| Barrel | 1×1 | 8 wood | 500 units of liquid. |
| Crate | 1×1 | 8 wood | 8 part stacks. |
| Basic lab | 2×2 | 20 wood + 4 copper plates + 4 clay bricks | Research with bronze kits. Analyzes samples. |

### 5.2 Tier 1: Steam

| Building | Size | Cost (summary) | What it does |
|---|---|---|---|
| Small boiler | 2×2 | bronze plates, bricks | Fuel (bulk input) + water (pipe) → steam (pipe). Uses 1 coal every 4 s and 6 water per second. Makes 6 steam per second. It explodes if it heats while dry and then gets water. |
| Steam crusher | 2×2 | bronze | Ore → crushed ore. 2 steam per second. |
| Steam hammer | 2×2 | bronze | Ingot → plate. |
| Steam wire drawer | 2×1 | bronze | Rod → 2 wires. |
| Steam furnace | 2×2 | bronze, bricks | Smelts dust and crushed ore up to 1300 °C. |
| Steam alloy smelter | 2×2 | bronze, bricks | Two metals → alloy. |
| Steam extractor | 2×2 | bronze | Rubber tree wood → resin. |
| Steam press | 2×2 | bronze | Rubber → sheets. Wood chips + resin → circuit board. |
| Steam washer | 3×2 | bronze | Crushed ore + water → washed ore + dirty water. |
| Steam assembler | 3×2 | bronze, iron | Parts → parts. |
| Steam pump | 1×2 | bronze | World fluid → pipe. |
| Steam drill | 3×3 | bronze, steel | Mines a fan-shaped area in front of it. Outputs the broken cells. |
| Steam blower | 1×1 | bronze | Blows hot air into a room (blast furnace). |
| Steam lab | 2×2 | bronze, glass | Research with bronze and steam kits. |
| Coke oven parts | 1×1 each | bricks | Walls, controller, hatch, coke door, creosote tap, gas outlet. |
| Blast furnace parts | 1×1 each | firebrick, iron | Walls, controller, top hatch, air inlet, iron tap, slag tap, exhaust. |
| Steel converter | 2×3 | firebrick, iron | Pig iron + air blast → steel + slag + CO₂. |
| Iron belt | 1×1 | iron | 16 cells per second. |
| Belt lift, screw lift | 1×N | iron | Move powder and parts up. |
| Splitter, sorter | 1×1 | iron | Split, filter. |
| Screen | 2×1 | iron | Sorts by grain size. |
| Magnet | 1×1 | iron, copper wire | Pulls magnetic powder. |
| Fan | 1×1 | iron | Pushes gas and light powder. |
| Arm | 1×1 | iron | Moves parts. |
| Iron crate | 1×1 | iron | 16 part stacks. |
| Bronze pipe, iron pipe | 1×1 | bronze, iron | See the pipe table in the game design. |
| Outlet, drain, valve, check valve, pipe bridge | 1×1 | iron | Fluid control. |
| Tank | 2×2 | iron | 4,000 units. |
| Large tank parts | 1×1 each | iron | Walls, controller, pipe ports. Any shape. |
| Signal wire, level sensor, temperature sensor, material sensor, lever, compare block | 1×1 | copper wire, iron | Signals. |
| Gate | 1×1 | iron | A door for powder or liquid. A signal opens it. |
| Sprinkler | 1×1 | iron | Water outlet. A signal opens it. |

### 5.3 Tier 2: LV

Steam turbine, combustion generator, water turbine, solar panel, battery box, tin cable (bare), insulated copper cable, macerator, electric furnace, wire mill, bender, lathe, cutter, ore washer, centrifuge, mixer, electrolyzer, chemical reactor, assembler, circuit assembler, arc furnace, electric pump, electric drill, simple distillery, item tube and tube parts, sweeper drone and dock, heat pipe, LV lab, signals II (math, memory, display).

### 5.4 Tier 3: MV

Gas turbine, geothermal generator, large boiler (room), MV transformer, distillation tower (room), electric blast furnace (room) with cupronickel coils, aluminium cell (room), air separation plant, implosion compressor, laser engraver, polymerizer, logistic drones and ports, stainless steel pipes, PVC pipes, MV versions of LV machines, cooling jacket upgrade.

### 5.5 Tier 4: HV

Fission reactor (room), large turbine, vacuum freezer, Kroll reactor (titanium), tungsten line machines, HV transformer, kanthal coils for the electric blast furnace, lead shielding, tungsten pipes, heat shield upgrade.

### 5.6 Tier 5: EV

Fusion reactor (room), float stone refinery, crystal charger, nanite assembler, core drill, core tap (room).

## 6. Key recipes (Tier 0 and Tier 1)

| Recipe | Where | Inputs | Outputs | Condition or time |
|---|---|---|---|---|
| Clay brick | kiln | raw clay brick | clay brick | ≥ 900 °C, 20 s |
| Charcoal | kiln or any sealed hot space | wood cells | charcoal cells | ≥ 300 °C, no air |
| Firebrick | kiln with bellows | raw firebrick | firebrick | ≥ 1200 °C, 30 s |
| Tin | kiln, crucible or campfire | cassiterite + charcoal | molten tin | ≥ 900 °C |
| Copper | kiln with bellows | malachite + charcoal | molten copper | ≥ 1100 °C |
| Bronze | crucible | 3 molten copper + 1 molten tin | 4 molten bronze | ≥ 1000 °C |
| Casting | mold | 16 to 48 molten metal | one part | cools below the freezing point |
| Crushing | stamp mill, crusher | 16 raw ore | 16 crushed ore; 10% chance of byproduct dust | 4 s (stamp mill) |
| Washing | sluice, washer | crushed ore + water | washed ore + light waste | depends on water flow |
| Coke | coke oven | 32 coal | 24 coke + 8 creosote + coal gas | 60 s, no air |
| Pig iron | blast furnace | iron ore + coke + limestone | molten pig iron + molten slag + CO₂ | ≥ 1400 °C with an air blast |
| Steel | steel converter | 64 molten pig iron + air | 60 molten steel + 4 slag + CO₂ | 10 s |
| Glass | kiln or steam furnace | sand + ash | molten glass | ≥ 1100 °C |
| Resin | steam extractor | rubber tree wood | resin | 5 s |
| Rubber | any heat | resin | rubber | ≥ 150 °C |
| Treated wood | barrel or tank | wood block + creosote | treated wood block | 10 s |
| Cement | mixer (Tier 2) or hand | quicklime + crushed slag | cement | – |
| Bronze kit | workbench, steam assembler | bronze gear + clay brick + tin plate | 1 bronze kit | 5 s |
| Steam kit | steam assembler | steel plate + bronze pipe section + glass vial | 1 steam kit | 8 s |

## 7. Production chains

### A. Tin and bronze (Tier 0)

1. Dig cassiterite gravel from a river bed. Dig malachite near the surface.
2. Burn wood in a sealed kiln to make charcoal.
3. Heat cassiterite and charcoal in a crucible over a campfire. Tin melts at a low temperature, so this works first.
4. Pour the tin into molds.
5. Build bellows (they need tin plates). Put them on the kiln. The kiln now reaches 1100 °C.
6. Smelt malachite with charcoal in the kiln. Tap the molten copper into a crucible.
7. Add tin: 3 copper + 1 tin makes bronze.
8. Cast bronze plates and gears.

### B. Iron and steel (Tier 1)

1. Research the bronze drill head. Dig into the upper stone.
2. Mine magnetite or hematite. Crush it.
3. Optional: a magnet pulls magnetite out of the crushed rock. This gives a cleaner ore.
4. Make coke in the coke oven. Collect the creosote and the coal gas.
5. Mine and crush limestone.
6. Feed ore, coke and limestone into the top of the blast furnace. Blow hot air in at the bottom.
7. Molten pig iron collects at the bottom. Molten slag floats on top of it.
8. Tap the iron from the low tap and the slag from the high tap. If the level is wrong, slag gets into the iron molds and the parts are scrap.
9. Blow air through the pig iron in the converter. This makes steel.

### C. Byproducts of iron (Tier 1)

- Slag → crushed slag → cement (with quicklime) → concrete.
- Creosote → treated wood (resists fire) or fuel.
- Coal gas → boiler fuel.
- CO₂ → vent it. It sinks and puts out fires in low rooms, so vent it high.

### D. Glass and rubber (Tier 1)

- Sand + ash (from burned wood) at 1100 °C → glass. Ash is a byproduct of Tier 0.
- Rubber tree wood (swamp) → resin (extractor) → rubber (at 150 °C) → rubber sheets.

### E. Chalcopyrite, sulfur dioxide and sulfuric acid (Tier 1 to Tier 2)

1. Crush and wash chalcopyrite.
2. Roast it in air. This makes roasted chalcopyrite and sulfur dioxide gas.
3. If the player vents the SO₂, it adds to air pollution and can cause acid rain.
4. If the player collects the SO₂ with a pipe, a chemical reactor (Tier 2) turns SO₂ + oxygen + water into sulfuric acid.
5. Sulfuric acid is needed for batteries and for ore acid baths.
6. Smelt the roasted chalcopyrite. This gives copper and iron dust.

### F. Brine electrolysis (Tier 2)

- Brine → electrolyzer → chlorine + hydrogen + sodium hydroxide solution.
- Chlorine → PVC plastic (Tier 3), titanium (Tier 4), hydrochloric acid.
- Hydrogen → fuel, tungsten reduction (Tier 4).
- Sodium hydroxide → aluminium (Bayer process, Tier 3), acid neutralizing.

### G. Oil (Tier 2 to Tier 3)

1. Find an oil pocket in the deep rock. Gas is on top, oil in the middle, brine at the bottom.
2. Tier 2: a simple distillery makes fuel, heavy oil and gas from crude oil.
3. Tier 3: the distillation tower. Heat at the bottom turns crude oil into vapors. The vapors rise and cool. Each one condenses at its own height. Taps at those heights collect the products. The controller sets the wall temperature at each height, so the result is stable.
4. Naphtha → cracking → ethylene → polyethylene. Ethylene + chlorine → PVC.

### H. Aluminium (Tier 3)

1. Bauxite + sodium hydroxide solution (from chain F) → alumina + red mud (waste that the player must store).
2. Alumina dissolved in molten cryolite in the aluminium cell (room machine).
3. A large electric current makes molten aluminium. It sinks to the bottom of the cell.

### I. Titanium (Tier 4)

Ilmenite + chlorine + coke → titanium tetrachloride (liquid) + iron byproduct. Titanium tetrachloride + magnesium (from brine) → titanium sponge + magnesium chloride. Recycle the magnesium chloride by electrolysis.

### J. Nuclear (Tier 4)

Uraninite + sulfuric acid → yellowcake → fuel rods. Fuel rods heat water in the reactor room. Spent rods are hot and radioactive and must be cooled in a water pool.

### Byproduct uses

| Byproduct | From | Use |
|---|---|---|
| Ash | burning wood | glass flux |
| Slag | smelting | cement, concrete |
| Creosote | coke oven | treated wood, fuel |
| Coal gas | coke oven | fuel |
| Carbon dioxide | burning, smelting, lime | vent; later fire suppression |
| Sulfur dioxide | roasting, burning coal and sulfur | sulfuric acid |
| Dirty water | washers | centrifuge → water + extra dust |
| Hydrogen | acid on metal, electrolysis | fuel, chemistry |
| Chlorine | brine electrolysis | PVC, titanium, hydrochloric acid |
| Sodium hydroxide | brine electrolysis | aluminium, neutralizing acid |
| Red mud | Bayer process | storage problem; gallium later |
| Heavy oil | distillation | lubricant, fuel |
| Radon | uraninite | none; vent it safely |

## 8. Research

Early Tier 0 technologies cost only discovery points, because the player has no kits yet. Later Tier 0 technologies cost bronze kits.

| Tier | Kit | Technologies |
|---|---|---|
| 0 | Bronze kit | Clay working, Charcoal, Tin casting, Bellows, Copper smelting, Bronze, Stamp mill, Sluice, Wood belts and hoppers, Basic lab, Bronze kit |
| 1 | Steam kit | Bronze drill head, Steam power, Steam machines I (crusher, hammer, furnace, wire drawer), Steam machines II (alloy smelter, washer, extractor, press, assembler), Coke oven, Blast furnace, Steel converter, Iron logistics (iron belt, lifts, arm), Sorting (splitter, sorter, screen, magnet, fan), Iron pipes and tanks, Signals I, Glass, Rubber, Treated wood, Steam drill, Steam kit |
| 2 | Electric kit | Electricity, Steel drill head, LV machines I, LV machines II, Electrolysis, Sulfuric acid, Batteries, Basic circuits, Oil I, Water turbine, Solar power, Item tubes, Sweeper drone, Heat pipe, Signals II, Electric kit |
| 3 | Chemical kit | Plastics, PVC, Aluminium, Distillation tower, Electric blast furnace, Stainless steel, Air separation, Implosion compressor (synthetic diamonds), Diamond drill head, Machine cooling, Geothermal power, Gas turbine, Logistic drones, Laser engraver, Good circuits, MV transformer, Chemical kit |
| 4 | Heavy kit | Titanium, Tungsten, Tungsten carbide drill head, Heat shield, Fission, Large turbine, Vacuum freezer, Microchips, Lead shielding, Kanthal coils, Heavy kit |
| 5 | Deep kit | Fusion, Float stone engineering, Charged crystals, Crystal processors, Nanites, Core drill, Core tap |

Research kit recipes:

| Kit | Recipe |
|---|---|
| Bronze kit | bronze gear + clay brick + tin plate |
| Steam kit | steel plate + bronze pipe section + glass vial |
| Electric kit | basic circuit + insulated wire + battery |
| Chemical kit | plastic sheet + vial of sulfuric acid + aluminium plate |
| Heavy kit | titanium plate + tungsten carbide + good circuit |
| Deep kit | crystal processor + float stone + heat-proof alloy plate |

## 9. Player upgrades

| Upgrade | Tier | Effect |
|---|---|---|
| Drill head: bronze, steel, diamond, tungsten carbide, core | 1–5 | Dig harder materials. |
| Dig radius I–III | 0–3 | Bigger dig circle. |
| Dig speed I–V | 0–4 | Faster digging. |
| Tank size I–V | 0–4 | More slots, more units per slot. |
| Heat-proof tank | 1 | Carry liquids above 300 °C, such as molten metal. |
| Jetpack I–III | 0–3 | Longer flight. |
| Heat shield I–III | 2–4 | Heat limit 80 → 200 → 450 → 800 °C. |
| Gas filter | 1 | Less damage from toxic gas. |
| Lamp I–II | 0–2 | Bigger light radius. |
| Reach I–III | 0–2 | Build from farther away. |
| Build speed I–III | 0–2 | Build ghosts faster. |
| Personal drones | 3 | Small drones that build ghosts near the robot. |
| Scanner I–III | 0–3 | Bigger scan radius. See ore and gas pockets through rock. |

## 10. Late-game materials with special rules

| Material | Rules | Use |
|---|---|---|
| Float stone | Its dust falls up. | Lifts that move powder up with no power. Floating platforms. |
| Charged crystal | Stores charge. Releases sparks when hit or heated. | Power storage. It also ignites gas, so it is dangerous near fuel. |
| Nanites | Turn metal cells into more nanites. Die above 1500 °C. | The nanite assembler makes complex parts from any metal dust. Keep nanites inside glass or ceramic. |
| Corium | Melts almost every material. Radioactive. | None. It is the result of a meltdown. |
| Plasma | About 10000 °C. | Fusion power. |
| Core matter | Very dense. About 8000 °C. Releases very much heat. | The final resource. |

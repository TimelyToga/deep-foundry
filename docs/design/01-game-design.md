# Deep Foundry: game design

## 1. Summary

Deep Foundry is a 2D side-view factory game. Every pixel of the world is one cell of a simulated material, as in Noita. Sand falls and makes piles. Water flows and finds its level. Fire spreads to wood and oil. Heat moves through walls. Metal melts, flows, and becomes solid again when it cools.

The player controls a small mining robot on an unknown planet. The robot digs, builds machines, and automates production. The goal is to go down through the layers of the planet to its core. Each layer is hotter than the layer above it. Each layer has new materials and new dangers. The player needs a new tier of technology to work in each layer.

What we take from other games:

| Game | What we take |
|---|---|
| Noita | Every material is physical. Materials react with each other. Fire, gas, liquids, explosions. |
| Factorio | Scale and throughput. Belts. Clear rates and numbers. Blueprints. Fast building. |
| GregTech: New Horizons (GTNH) | Long production chains. Byproducts. Voltage tiers. Multiblock machines. A recipe browser. A quest book. |

## 2. Design rules

These rules decide arguments about the design. If a feature breaks a rule, we change the feature.

1. **Physics is part of production.** Heat melts ore. Water washes light dirt away from heavy ore. Slag floats on molten iron. Gas rises and collects under a roof. The player uses these facts to process materials. The physics is never only decoration.
2. **Everything is made of real material.** Machines, pipes, gears, and the contents of tanks are all made of materials. When they break, burn or melt, the material goes back into the world as cells. A lava pipe that breaks releases real lava.
3. **Every process makes something extra.** Most recipes have byproducts. The player must use them, store them, or remove them. Removal has a cost: pollution, energy, or space.
4. **Failure is visible and has a clear cause.** When something goes wrong, the player can see what happened and why. Example: a boiler explodes because it ran dry while hot and then got cold water. The game shows this in the alert.
5. **Building is fast and precise.** The player can place, rotate, drag, copy, paste and undo with no delay. The game never makes the player wait for the build tools.
6. **Depth is progression.** Each new layer needs new technology and gives new materials. The world map is also a map of the tech tree.
7. **Numbers are visible.** The player can always see rates per minute, power use, temperatures, and the reason a machine stopped.

## 3. Problems in simple sand factory games, and our answers

You said Sandustry felt limited. These are the problems we design against.

| Problem | Our answer |
|---|---|
| Materials differ only in color and name. | Each material has real properties: density, melting point, boiling point, burn point, heat conduction, electrical conduction, hardness, corrosion. These properties decide how the player mines, moves, stores and processes it. |
| Machines are "put X in, get Y out". | Many processes use physics. Machines have byproducts, heat output, power limits and failure modes. Some machines are rooms that the player shapes (section 13). |
| Progression is short and flat. | Six tiers. Each tier adds a new power source, new machines, new materials, and a deeper layer. Recipes in later tiers need parts from all earlier tiers. |
| Logistics has no challenge. | Early transport is physical (belts, chutes, open channels). Later transport is sealed (pipes, tubes) and has limits: temperature, gas-tight or not, corrosion, throughput. Ratios matter. |
| The world is only a background. | The world reacts. Gas pockets explode. Underground water floods tunnels. Lava breaks through. Sand collapses. Pollution causes acid rain. |
| There is no reason to improve a working factory. | Longer ore processing gives more metal per ore and extra byproducts. Higher machine tiers run faster. Statistics show where the bottleneck is. |
| Discovery ends early. | The player finds reactions by experiment. Deep layers have materials with new rules, for example a stone that falls up. |

## 4. Core loops

**Short loop (minutes).** Find a material. Dig it. Process it by hand or in a simple machine. Use it to build something.

**Middle loop (one to three hours).** Automate a production chain. Feed research. Reach a milestone. Unlock the next tier.

**Long loop (the full game).** Go deeper. Each layer changes what the player can make and what can go wrong. The final goal is the planet's core.

A typical first hour:

1. Land on the surface. The landing pod is the base. We call it the **Hub**.
2. Dig sand, clay and dirt with the multi-tool. Cut a tree.
3. Find green copper ore (malachite) near the surface. Find tin ore (cassiterite) in river gravel.
4. Build a clay kiln room. Heat wood in it with no air to make charcoal.
5. Heat tin ore with charcoal. Pour the molten tin into a mold. This is the first metal part.
6. Build bellows to make the kiln hotter. Now copper ore melts too.
7. Mix copper and tin in a crucible to make bronze.
8. Build a water-driven stamp mill to crush ore. Build a sluice to wash it. Now each ore gives more metal.
9. Deliver bronze parts to the Hub. This unlocks the Steam tier.

## 5. The world

### 5.1 Units of size

| Unit | Size | Use |
|---|---|---|
| Cell | 1 × 1 | One cell holds one material. One cell of a material is one **unit** of that material. |
| Tile | 8 × 8 cells | Buildings snap to tiles. |
| Chunk | 64 × 64 cells (8 × 8 tiles) | The simulation updates, sleeps and saves per chunk. The renderer uploads per chunk. |

- Default world: 8192 cells wide and 8192 cells deep (1024 × 1024 tiles, 128 × 128 chunks). The sides and the bottom are bedrock. The sky is open above the surface.
- At the default zoom, one cell is 3 × 3 screen pixels. The player can zoom from 8 screen pixels per cell to 1 screen pixel per cell. A map view shows the whole world.
- The player robot is about 8 cells wide and 16 cells tall (1 × 2 tiles).

### 5.2 Layers

Depth is in cells below the surface.

| # | Layer | Depth | Air temperature | Rock | New materials | Dangers | Needed to dig here |
|---|---|---|---|---|---|---|---|
| 0 | Surface | sky to 600 | 15 °C (changes by biome) | dirt, sand, clay, gravel | malachite, cassiterite gravel, coal outcrops, wood, water | fire, rain, lightning | nothing |
| 1 | Upper Stone | 600–1800 | 20 °C | stone, limestone | magnetite, hematite, chalcopyrite, coal seams | methane pockets, underground water, sand collapse | bronze drill head (Tier 1) |
| 2 | Deep Rock | 1800–3600 | 40–80 °C | granite | galena, sphalerite, gold, sulfur, salt, bauxite, quartz, crude oil, brine | hydrogen sulfide, gas under pressure, hot springs | steel drill head (Tier 2) |
| 3 | Magma | 3600–5400 | 150–400 °C | basalt, obsidian | wolframite, chromite, pentlandite, ilmenite, kimberlite (diamonds) | lava lakes and flows, heat, sulfur dioxide vents | diamond drill head and machine cooling (Tier 3) |
| 4 | Crystal Deep | 5400–7200 | 300–600 °C | crystal rock | uraninite, monazite, float stone, charged crystals | radiation, radon gas, crystals that release sparks | tungsten carbide drill head and heat shield (Tier 4) |
| 5 | Core | 7200–8192 | 800 °C and more | core rock | core matter | extreme heat | core drill (Tier 5) |

All numbers in this document are starting values for balance. They live in data files.

### 5.3 Surface biomes

The surface extends left and right from the start area. Each biome continues down into the upper layers with its own features.

| Biome | Position | Features | What it teaches |
|---|---|---|---|
| Temperate (start) | center | grass, trees, clay, lakes, river gravel with tin, malachite | the basics |
| Desert | one side | sand dunes, salt flats (halite), no trees, little water | glass, salt, water is scarce |
| Tundra | other side | snow, ice, frozen ground, air at −20 °C | water freezes in pipes; heat is needed |
| Swamp | past the temperate zone | mud, peat, methane bubbles, sulfur springs, rubber trees | gas danger, rubber, fuel |
| Volcanic | far edge | basalt, lava vents near the surface, sulfur | early heat, early danger |

### 5.4 Sky and weather

- Day and night: a 20-minute cycle. Solar power works only in daylight.
- Rain falls as real water cells. It fills lakes, puts out fires, and makes bare cables short-circuit.
- Snow falls in the tundra and forms piles.
- Lightning hits high points. It sets trees on fire. Later, lightning rods collect it for power.
- Gas that rises to the top of the sky leaves the world. It adds to the **air pollution** value. High air pollution causes smog (less solar power) and then acid rain (damages exposed metal). Air pollution goes down slowly. Trees make it go down faster.

### 5.5 World generation

- A seed makes the world. The same seed always makes the same world.
- Caves, ore veins and pockets depend on the layer.
- Ores form in veins. Some ores form in special places:
  - Cassiterite (tin ore) collects in gravel at the bottom of rivers and lakes, because it is heavy. This is also true in reality.
  - Oil pockets have layers: gas at the top, oil in the middle, brine at the bottom. If the player drills into the top, gas comes out under pressure.
  - Diamonds form in vertical kimberlite pipes in the magma layer.
- The start area always has trees, clay, water, sand, malachite, cassiterite gravel and a coal outcrop within 150 tiles of the Hub.

## 6. The player

### 6.1 Movement

- Walk, run, jump.
- Jetpack: short flight. It uses energy and recharges on the ground. The exhaust is real hot gas. It can set dry grass on fire.
- Swim slowly in liquids. Very dense liquids (mercury, molten metal) push the robot up.
- Climb ladders and scaffolds.

### 6.2 Health and damage

- The robot has **hull points**. Damage comes from heat above its limit, toxic or corrosive gas, acid, explosions, long falls and radiation.
- The robot has a **heat limit**. At the start it is 80 °C. Upgrades make it higher. The magma layer needs cooling upgrades.
- At zero hull points, the robot is rebuilt at the Hub. The inventory stays in a fireproof crate at the place of death.

### 6.3 The multi-tool

The multi-tool is always in the robot's hand. It has these modes:

| Mode | Input | What it does |
|---|---|---|
| Dig | hold left mouse | Breaks cells in a circle at the cursor. The broken cells fly to the robot and go into the material tank. Radius, speed and maximum hardness depend on upgrades. |
| Spray | hold right mouse | Puts material from the tank back into the world, as a stream or one cell at a time. |
| Build | select a building | Places buildings and ghosts (section 18). |
| Remove | hold X | Takes buildings back into the inventory. |
| Scan | hold F | Shows the material name, temperature and properties under the cursor. The first scan of a new material gives research (section 15). |
| Ignite | tool slot | Makes a small flame. It starts kilns and furnaces. |

Hardness classes for digging: soft (sand, dirt, clay, snow, surface ores), stone, hard (granite), very hard (basalt, obsidian), extreme (crystal rock, core rock). Each class needs a better drill head.

### 6.4 Inventory

- **Material tank.** Holds bulk material in units. It starts with 4 slots of 2,000 units. Each slot holds one material. Without the heat-proof tank upgrade, the robot cannot collect liquids hotter than 300 °C.
- **Part slots.** Hold stacks of parts (gears, plates, circuits) and buildings in item form.
- **Hotbar.** 10 slots for buildings and tools.

### 6.5 Hand crafting

The robot can make simple parts and buildings from its inventory. Hand crafting is slow. A workbench makes it faster. Machines are much faster. Hand crafting exists so that the player is never stuck.

## 7. Materials

The full lists are in `02-content.md`. This section gives the rules.

### 7.1 Phases

| Phase | Behavior |
|---|---|
| Solid | Does not move. Can break, melt, burn or dissolve. Rock, wood, metal blocks, ice, glass. |
| Powder | Falls. Makes piles with a slope. Sinks in liquids with a lower density. Sand, gravel, crushed ore, coal, ash, snow. |
| Liquid | Falls and spreads to the sides. Liquids form layers by density: the densest liquid goes to the bottom. Water, oil, acid, lava, molten metal. |
| Gas | Rises if lighter than air. Sinks if heavier than air. Spreads out. Some gases fade over time. Steam, smoke, methane, chlorine. |
| Fire | Has a short life. Moves up. Heats and ignites what it touches. |
| Special | Has its own rules. Sparks, plasma, float stone, nanites. |

Heavy gases matter. Carbon dioxide, chlorine and hydrogen sulfide sink and collect in low tunnels. Carbon dioxide puts out fires. Chlorine and hydrogen sulfide damage the robot.

### 7.2 Properties

Each material has these properties. They are defined in data files.

- **Density.** Decides what sinks and what floats.
- **Flow.** For liquids: how fast they spread (water is fast, lava is slow, mud is very slow). For powders: how steep the piles are.
- **Grain size.** For powders: small, medium or large. Screens (sieves) sort powders by grain size.
- **Hardness.** How hard it is to dig, and how well it resists explosions.
- **Heat capacity** and **heat conduction.**
- **Phase change points.** Melting, freezing, boiling and condensing temperatures, and the material it becomes.
- **Burning.** Ignition temperature, burn speed, heat released, and what it becomes (ash, smoke, gas).
- **Broken form.** What a solid becomes when it is dug or blasted (stone becomes gravel, an ore vein becomes raw ore).
- **Electrical conduction.** Conductive materials carry sparks and can short-circuit bare cables.
- **Corrosion.** How strongly it attacks other materials (acids), and how well it resists attack.
- **Toxicity** and **radioactivity.**
- **Magnetic.** Magnets pull magnetic powders.
- **Light.** Some materials glow. All materials glow when they are very hot (section 20).
- **Colors.** A small set of colors. Each cell gets one fixed shade from this set, so piles look textured.

### 7.3 Discovery

The game does not show every reaction at the start.

- When a reaction happens near the player for the first time, a message appears. Example: "New reaction: mercury + gold dust → gold amalgam." The reaction goes into the recipe browser.
- A scan of a new material also adds it to the browser.
- Some research needs discoveries. Example: "Acid chemistry" needs a scan of sulfur and one observed reaction of acid with a metal.

## 8. Heat

Every cell has a temperature.

- Heat flows from hot cells to cold cells. Metal conducts heat fast. Firebrick and air conduct heat slowly.
- Empty cells (air) slowly move toward the air temperature of their layer.
- Hot gas carries its heat when it moves up. This gives natural convection.
- When a cell passes a phase change point, it changes material. Ice melts into water. Water boils into steam. Steam condenses into water when it cools. Sand melts into molten glass.
- Burning and some reactions add heat. Some reactions remove heat (liquid nitrogen boils and cools what it touches).
- Each building has a **maximum temperature**. Above it, the building stops. Far above it, the building takes damage.

Heat is a main gameplay system:

- A furnace is a hot room. Good walls (firebrick) keep the heat in. Bad walls let it out and heat the machines next to them.
- Heat exchangers move heat from lava into water to make steam.
- Cooling loops keep machines working in the magma layer.
- In the tundra, a water pipe outside freezes unless the player heats it.

## 9. Reactions

A reaction happens when two materials touch, or when one material meets a condition.

Each reaction rule has:

- Input A and input B. An input is a material, a material tag (for example "any metal"), or nothing.
- Conditions: a temperature range, and sometimes "needs air next to it" or "needs a spark".
- Outputs: what A and B become.
- A chance per tick. This sets the speed of the reaction.
- A heat change.

Examples. The full list is in `02-content.md`.

| Inputs | Condition | Result |
|---|---|---|
| Water + lava | — | Steam + obsidian |
| Wood | 300 °C or more, air next to it | Fire, then ash and smoke |
| Wood | 300 °C or more, no air next to it | Charcoal |
| Methane | touches fire or a spark | Explosion |
| Sulfuric acid + iron | — | Iron sulfate solution + hydrogen gas. The iron dissolves. |
| Cement + water | — | Wet concrete. After 30 seconds it becomes concrete. |
| Sand | 1700 °C or more | Molten glass |
| Salt + ice | — | Brine. The ice melts. |
| Malachite + charcoal | 1100 °C or more | Molten copper + carbon dioxide |
| Molten metal + water | — | Steam burst (a small explosion) + solid metal |

Reactions happen anywhere in the world. Machines are only one place for them. If the player drops iron ore and coke into a natural lava lake, some iron forms. The yield is poor and the player cannot control it. Machines and room machines give better yield and control.

## 10. Buildings: general rules

### 10.1 Two layers

- **Front layer.** Machines, belts, walls, hoppers, tanks. Front-layer buildings are solid. Cells cannot move through them.
- **Back layer.** Pipes, power cables, item tubes and signal wires. They are behind the cells. Cells move in front of them freely. The player can put back-layer pieces behind rock.

Back-layer pieces still react with the cell in front of them:

- A hot cell heats the pipe or cable behind it.
- An acid cell corrodes the pipe behind it.
- A water cell in front of a bare cable causes a short circuit: sparks and power loss.

The player presses **Tab** to see and edit the back layer. When the player selects a back-layer item, the view changes to the back layer automatically.

### 10.2 Buildings are made of material

- Each building type has a body material (for example, "bronze casing"). The body cells are in the world as solid cells.
- Each building has hit points and a maximum temperature.
- Explosions, heat, corrosion and overvoltage damage buildings.
- When a building is destroyed, it releases its contents (the fluid in a tank, the ore in a hopper, the molten metal in a crucible) and some scrap.

### 10.3 Ports

A port is the place where a building exchanges material with the world or with another building.

| Port type | What it does |
|---|---|
| Bulk input | Takes powder cells that fall or slide into it. It has a filter. It does not take materials that the machine cannot use; those cells stay outside. |
| Bulk output | Puts powder cells into the world in front of it, if there is space. |
| Fluid input | Takes liquid or gas cells from the world in front of it. |
| Fluid output | Puts liquid or gas cells into the world. |
| Pipe connection | Connects to a pipe in the back layer. |
| Part input / output | Takes or gives parts (discrete items). |
| Tube connection | Connects to an item tube in the back layer. |
| Power | Connects to a cable. |
| Heat contact | Takes heat from, or gives heat to, the cells that touch this side. |
| Exhaust | Releases waste gas (smoke, carbon dioxide, steam) into the world. |

If an output port is blocked, the machine stops and shows "Output blocked". If an exhaust is blocked, the machine stops. Some machines also take damage.

### 10.4 Support

Buildings do not need support. They can float in the air. The game draws simple legs under floating buildings as decoration. Loose terrain (sand, gravel) can still fall away around them.

## 11. Logistics

Logistics moves four kinds of things: bulk material (powders), parts, fluids (liquids and gases), and power. Signals control them.

### 11.1 Bulk material (powders)

| Building | Tier | What it does |
|---|---|---|
| Wall and slope blocks | 0 | Solid blocks and 45° slopes. They guide falling powder. |
| Chute | 0 | A narrow sloped channel. |
| Hopper | 0 | Collects powder from above. Releases it at a set rate. Can filter. |
| Belt | 0–4 | Moves cells and parts that rest on it. Wood belt (slow, can burn), iron belt, steel belt, alloy belt. |
| Belt lift | 1 | A closed lift. Takes powder or parts at the bottom. Releases them at the top. |
| Screw lift | 1 | A closed tube that moves powder up at an angle. |
| Splitter | 1 | Sends its input to two outputs in turn, or by a set ratio. |
| Sorter | 1 | Sends one chosen material one way, and all other materials the other way. |
| Screen (sieve) | 1 | Small grains fall through it. Large grains roll off it. |
| Magnet | 1 | Pulls magnetic powder to it. Drops it when switched off. |
| Fan | 1 | Pushes gas and light powder. It blows light dust away from heavy ore. It also pushes fire. |
| Sweeper drone | 2 | Collects loose cells in an area and takes them to storage. |

**How belts move material.** At each belt step, every column of powder that rests on the belt moves one cell in the belt direction. A pile moves as one piece. At the end of the belt, the material falls off. Belt throughput depends on belt speed and pile height.

### 11.2 Parts (discrete items)

Parts are objects like gears, plates, wires and circuits. In the world, a part is a small object (about 4 × 4 cells) with simple physics. Parts fall, rest on belts, make piles, float or sink, burn, and melt. Each part is made of a material. If a copper gear falls into lava, it melts into molten copper cells.

| Building | Tier | What it does |
|---|---|---|
| Crate | 0 | Stores parts. |
| Arm | 1 | Picks up parts in one place and puts them in another place. Can filter. |
| Item tube | 2 | A back-layer tube. Moves parts between tube connections. Routes parts by filter. |
| Logistic drones | 3 | Fly parts between storage and machines when a machine requests them. |

### 11.3 Fluids (liquids and gases)

| Building | Tier | What it does |
|---|---|---|
| Open channel | 0 | Walls that guide liquid by gravity. Fully physical. |
| Barrel | 0 | Small liquid storage. |
| Pump | 1 | Takes fluid cells from the world into a pipe. |
| Pipe | 1–5 | A back-layer pipe. Moves fluid between connections. |
| Outlet | 1 | Puts fluid from a pipe into the world. |
| Drain | 1 | A grate in the floor. Takes liquid that flows onto it into a pipe. |
| Tank | 1 | A fixed tank. Large tanks are room machines of any shape (section 13). |
| Valve | 1 | Opens or closes a pipe. A signal can control it. |
| Check valve | 1 | Lets fluid move one way only. |
| Pipe bridge | 1 | Takes a pipe over other pipes, up to 8 tiles. |
| Heat pipe | 2 | Moves heat between its ends. It carries no fluid. |

**Pipe rules.**

- One connected pipe system holds one fluid at a time. The game warns the player before two systems with different fluids connect.
- Each pipe material has a **maximum temperature**, a **gas-tight** flag, and **acid resistance**.

| Pipe | Max temperature | Gas-tight | Acid-proof | Tier |
|---|---|---|---|---|
| Bronze | 600 °C | no | no | 1 |
| Iron | 800 °C | yes | no | 1 |
| Steel | 1200 °C | yes | no | 2 |
| PVC plastic | 150 °C | yes | yes | 3 |
| Stainless steel | 1500 °C | yes | yes | 3 |
| Tungsten | 3200 °C | yes | yes | 4 |

- A pipe that is too hot takes damage and then breaks.
- Gas in a pipe that is not gas-tight leaks slowly into the world.
- Acid damages pipes that are not acid-proof.
- A broken pipe releases its fluid into the world at the break.

### 11.4 Signals

Signals control machines, valves, gates, belts and pumps.

- Signal wires are in the back layer.
- A signal is a number on a wire.
- Sensors make numbers: level sensor (units in a tank or room), temperature sensor, material sensor ("is material X in front of me?"), power sensor, counter.
- Machines, valves, gates, belts and pumps have an enable condition. Example: "run when the signal is less than 500".
- Logic blocks: compare, add, AND, OR, timer, memory.

Example: open the furnace tap when the temperature is above 1200 °C and the molten iron level is above 100 units.

## 12. Machines

### 12.1 Parts of a machine

Each machine has:

- A size in tiles and a body material.
- Ports (section 10.3).
- A tier. The tier sets the power input and the recipes it can run.
- Recipes. Each recipe has inputs, outputs, byproducts with chances, time and energy.
- A maximum temperature and a heat output. Most machines release some heat.
- Internal buffers with limits.
- Up to two upgrade slots. Examples: heat shield, gas seal, cooling fins.
- A status: working, no input, output blocked, no power, too hot, wrong voltage, room not valid.

### 12.2 Tier overclocking

A machine can run recipes of its own tier and of lower tiers. For each tier above the recipe's tier, the machine runs 2× faster and uses 4× the power. So a higher-tier machine is faster but less efficient. This is the GTNH rule.

### 12.3 Machine families

Each family has versions for several tiers. The full list is in `02-content.md`.

| Family | What it does |
|---|---|
| Crusher | Breaks ore into crushed ore. Has a chance of an extra byproduct. |
| Washer | Washes crushed ore with water. Light waste leaves with dirty water. |
| Furnace | Heats its input to a set temperature. |
| Alloy smelter | Melts two metals together in a set ratio. |
| Press / hammer | Makes plates. |
| Wire mill, lathe, cutter, bender | Make wires, rods, bolts and shaped parts. |
| Assembler | Combines parts into new parts. |
| Centrifuge | Separates mixtures by density. |
| Electrolyzer | Splits liquids with electric power. Example: water into hydrogen and oxygen. |
| Chemical reactor | Combines fluids and powders into new materials. |
| Mixer | Mixes powders. Example: cement. |
| Drill | Mines the cells in front of it and outputs them. |
| Lab | Does research with research kits. |

## 13. Room machines

A room machine is a machine whose body is a room that the player builds. It is our version of the GTNH multiblock. In a side view, the player can see the room and shape it easily.

How it works:

1. The player builds a closed room from the correct wall block (for example, firebrick).
2. The player puts a **controller** block in the wall.
3. The player puts port blocks in the wall: input hatches, output taps, air inlets, exhausts, heaters.
4. The controller checks the room: the walls are closed, the wall material is correct, the size is inside the limits, and the required blocks are there. It marks errors on the walls in red.
5. The inside of the room is part of the normal simulation. The materials inside are real cells. The player can see them melt, flow, burn and form layers.

The controller does three things:

- It shows the state of the room: temperature, contents, levels.
- It gives a bonus to reactions inside the room: better yield, less waste.
- It controls the port blocks of the room.

Size matters. A bigger room holds more and makes more, but it needs more heat.

| Room machine | Tier | Walls | What happens inside |
|---|---|---|---|
| Kiln | 0 | clay brick | Fire from wood or charcoal. Bakes clay into brick. Makes charcoal. Smelts tin, and copper with bellows. |
| Coke oven | 1 | brick | Coal is heated with no air. It makes coke, creosote (a liquid) and coal gas. |
| Blast furnace | 1 | firebrick | Ore, coke and limestone go in at the top. Hot air goes in at the bottom. Molten iron collects at the bottom. Slag floats on the iron. The player puts one tap low (for iron) and one tap higher (for slag). |
| Large tank | 1 | iron or steel | Holds real fluid cells. The level is visible. Any shape. |
| Large boiler | 2 | steel and firebrick | Large steam output. |
| Distillation tower | 3 | steel | A tall room. Heat is at the bottom. Hot crude oil vapor rises. Each fraction condenses at its own height. Taps at different heights collect different products. A taller tower separates more fractions. |
| Electric blast furnace | 3 | heat-proof casing and heating coils | Very high temperatures for steel alloys, titanium and tungsten. The coil type sets the maximum temperature. |
| Aluminium cell | 3 | carbon lining | Molten cryolite with alumina in it. A large electric current flows through it. Molten aluminium sinks to the bottom. |
| Fission reactor | 4 | reinforced concrete and steel | Fuel rods heat water to make steam. Control rods set the power. If cooling fails, the core melts. Corium (molten core) melts through almost everything. |
| Fusion reactor | 5 | field coils | A field holds the plasma. If the field loses power, the plasma escapes. |
| Core tap | 5 | core casing | The final machine. |

## 14. Power

| Tier | Name | Power sources |
|---|---|---|
| 0 | Hand and Fire | Fire (heat only). Flowing water for the stamp mill. |
| 1 | Steam | Boilers make steam. Steam machines take steam directly from pipes. |
| 2 | LV (low voltage) | Steam turbine, combustion generator, water turbine, solar panel, battery. |
| 3 | MV (medium voltage) | Gas turbine, geothermal generator (heat from lava), large boiler with turbine. |
| 4 | HV (high voltage) | Fission reactor, large turbine. |
| 5 | EV (extreme voltage) | Fusion reactor. |

Electric rules:

- Each voltage tier is 4× the voltage of the tier below: LV 32 V, MV 128 V, HV 512 V, EV 2048 V.
- Cables connect generators, batteries and machines into a network.
- Each cable type has a **voltage limit** and a **current limit** (amps).
- A network can carry the current limit of its weakest cable. If the current is higher, the weakest cables get hot. The heat goes into the cells around them. If they get too hot, they melt and the network breaks.
- If a machine gets more voltage than its tier, it explodes. The build tool shows a red warning before the player connects it.
- Transformers change the voltage one tier up or down.
- Bare cables short-circuit when water touches them. Insulated cables (rubber, later plastic) do not.
- Batteries store power. A power panel shows production, use and stored power over time.

## 15. Research and progression

Progression has four parts: milestones, the tech tree, discovery and the guide.

### 15.1 Milestones (tier gates)

The Hub (the landing pod) is damaged. Each repair stage needs a delivery of parts and materials. Each completed stage unlocks the research for the next tier. This gives the player one clear goal per tier.

| Stage | Unlocks | Example delivery |
|---|---|---|
| 1 | Tier 1: Steam | 40 bronze plates, 20 bronze gears, 100 clay bricks |
| 2 | Tier 2: LV | 100 steel plates, 200 copper wires, 50 glass panes, 20 rubber sheets |
| 3 | Tier 3: MV | 100 basic circuits, 50 batteries, 200 insulated wires, 20 barrels of sulfuric acid |
| 4 | Tier 4: HV | aluminium, stainless steel, good circuits |
| 5 | Tier 5: EV | titanium, tungsten carbide, microchips |
| 6 | Win | core matter delivered to the surface |

### 15.2 Tech tree

- Research happens in labs.
- Each technology costs research kits and time.
- Each tier has one research kit type. A kit is a part made from that tier's materials. Later technologies need kits from all earlier tiers, so the player must keep old production lines working.
- Technologies unlock buildings, recipes, player upgrades and global bonuses (belt speed, mining yield).
- After the final tier, repeatable technologies give small bonuses with no end.

| Kit | Tier | Made from (example) |
|---|---|---|
| Bronze kit | 0 | bronze gear, clay brick, tin plate |
| Steam kit | 1 | steel plate, bronze pipe, glass vial |
| Electric kit | 2 | basic circuit, insulated wire, battery |
| Chemical kit | 3 | plastic sheet, acid vial, aluminium plate |
| Heavy kit | 4 | titanium plate, tungsten carbide, good circuit |
| Deep kit | 5 | crystal processor, float stone, heat-proof alloy |

### 15.3 Discovery

- A scan of a new material, or a new reaction seen for the first time, gives **discovery points** and adds entries to the recipe browser.
- Some technologies need discoveries as well as kits.
- This rewards exploration and experiments.

### 15.4 Guide

A guide (like the GTNH quest book) lists goals for each tier with short hints. Completed goals give small rewards. The guide is also the tutorial.

### 15.5 Play time targets

| Tier | Time for a normal player |
|---|---|
| 0 | 1–2 hours |
| 1 | 3–5 hours |
| 2 | 6–10 hours |
| 3 | 10–15 hours |
| 4 | 15–20 hours |
| 5 | 20 hours or more |

## 16. Ore processing depth

The player can process ore with a short method or a long method. The long method gives more metal and more byproducts.

One unit of ore contains 1.5 units of metal. Simple methods lose most of it.

| Method | Tier | Metal from 100 units of ore | Byproducts |
|---|---|---|---|
| Smelt raw ore | 0 | 50 | slag |
| Crush, then smelt | 0 | 70 | slag, small chance of a second metal |
| Crush, wash, then smelt | 0–1 | 90 | the second metal as dust |
| Crush, wash, centrifuge, then smelt | 2 | 110 | two byproducts |
| Crush, wash, acid bath, centrifuge, electrolytic refining | 3 | 140 | rare metals (gold, silver, rare earths) |

Each ore has its own byproducts. Chalcopyrite (copper) gives iron and sulfur. Galena (lead) gives silver. Sphalerite (zinc) gives cadmium and gallium.

## 17. Hazards and pollution

| Hazard | Cause | What the player sees | How to handle it |
|---|---|---|---|
| Fire | Hot machines, sparks, lightning, lava | Flames, smoke | Build with materials that do not burn. Firebreaks. Water. Sprinklers on a temperature sensor. |
| Gas explosion | Methane or hydrogen meets fire or a spark | A flash, a crater, flying debris | Scan for gas before digging. Vent with fans. |
| Toxic gas | Roasting ore, sulfur vents, chlorine leaks | Colored gas that damages the robot | Exhaust stacks, scrubbers (turn sulfur dioxide into acid), filter upgrade. |
| Heavy gas in tunnels | Carbon dioxide, chlorine and hydrogen sulfide sink | Gas collects in low tunnels. Fires go out. | Fans, vents, sensors. |
| Flood | Digging into underground water | Water pours into tunnels | Walls, pumps, drains. |
| Collapse | Sand and gravel above a tunnel | Material falls | Walls, concrete. |
| Lava | Magma layer, volcanic biome | Glowing liquid that melts and burns | Heat-proof walls, water to make obsidian, cooling. |
| Boiler explosion | A boiler runs dry while hot, then gets water | Explosion, steam | Keep the water supply stable. Level sensors. |
| Overvoltage | Wrong cable or transformer | The machine explodes | Build tool warning. |
| Cable overload | Too much current | Hot cables, then melting | Better cables, more lines. |
| Pipe failure | Fluid too hot, acid, or gas in a pipe that is not gas-tight | Leaks | The correct pipe material. |
| Acid rain | High air pollution | Grey-green rain; exposed metal corrodes | Scrub exhaust. Build roofs. Plant trees. |
| Radiation | Uranium ore, radon gas, reactors | Glow; damage to the robot and electronics | Lead walls, distance. |
| Meltdown | A reactor loses cooling | Corium melts down through floors | Backup cooling, control rods on sensors. |
| Nanite escape | Late-game nanites leak out | Grey material that eats metal and makes more of itself | Keep nanites inside glass or ceramic. Destroy them with heat. |

The first version has no enemy creatures. The world itself is the danger. We can add creatures later.

Pollution:

- Machines release exhaust gas as real cells.
- In caves, light gas collects under the roof and heavy gas collects on the floor.
- On the surface, gas rises and leaves the world at the top of the sky. It adds to air pollution.
- Air pollution causes smog and acid rain (section 5.4).

## 18. Construction

Building must be fast and smooth. These points are requirements.

### 18.1 Placement

- Buildings snap to the tile grid. A **ghost** (a see-through preview of the building) follows the cursor.
- The ghost is green if the player can place it and red if not. A red ghost shows the reason in text. Examples: "Blocked by stone: dig first", "Needs 4 more bronze plates", "Wrong voltage: this cable is MV".
- The ghost shows its ports and flow directions as small arrows.
- **R** rotates. **F** flips.
- **Drag** places a line. Belts turn to follow the drag. Pipes and cables connect automatically.
- **Shift + drag** places a line with gaps.
- **Q** picks the building under the cursor (same type and rotation), as in Factorio.
- **Ctrl + Z** and **Ctrl + Y** undo and redo build and remove actions.
- **Ctrl + C** and **Ctrl + V** copy and paste an area. The paste can rotate and flip.
- **Blueprints** save copied areas in a library.
- **Upgrade tool**: drag over an area to replace buildings with a higher tier.
- Loose cells in the footprint (powder, liquid, gas) are pushed out of the way. Solid cells must be dug first. With **auto-dig** on, a ghost over solid cells marks those cells for digging.

### 18.2 Ghosts and building

- If the player does not have the items, the placement becomes a ghost. The robot (and later construction drones) builds the ghost when the items are available.
- The robot builds ghosts in its reach automatically, quickly, one after another, with a short animation.
- The player can plan with ghosts while the game is paused.

### 18.3 Removal

- Hold the remove key on a building to remove it. Drag a rectangle to remove many.
- Removed buildings go back to the inventory with their contents. If the inventory is full, the contents drop as cells.

### 18.4 Views and overlays

- **Tab**: back-layer view (pipes, cables, tubes, signal wires).
- **Alt**: info view. Shows recipe icons on machines, belt directions and pipe contents.
- Overlays: heat, gas, power networks, pollution.
- Hover a cell: material name and temperature.
- Hover a building: status, recipe, rate.

### 18.5 Feel

- The game reads input every frame. Placement never waits for the simulation.
- The ghost follows the cursor with no delay.
- A new building appears with a short animation and some dust. A removed building falls apart into particles.
- Each action has a sound.

## 19. Interface

- **HUD**: hull points, heat, tank contents, hotbar, minimap, alerts.
- **Alerts**: fire, leak, machine stopped, low power, gas, flood. A click on an alert shows its location.
- **Machine panel**: recipe choice, input and output buffers, progress, power, temperature, status and reason.
- **Recipe browser** (like NEI in GTNH): search any material or part. It shows "how to make it" and "what uses it", with machine, tier, time and rate per minute. It includes reactions. Hover an item and press **R** for recipes or **U** for uses.
- **Tech tree**: a graph that the player can zoom and move.
- **Production statistics**: amount made and used per minute for each material, over 1 minute, 10 minutes, 1 hour and 10 hours. Power graphs.
- **Map**: the explored world, scan results, buildings, alerts.
- **Guide**: goals for each tier.

## 20. Graphics, animation and sound

### 20.1 Style

- Pixel art. One art pixel is one cell. Machines are drawn at the same scale as the world.
- We start with simple shapes and a fixed color set. We replace the art later. The code must make art easy to replace.

### 20.2 World rendering

- Each material has a few colors. Each cell keeps one random shade for its life.
- Liquid colors shift a little over time, so the liquid looks like it moves.
- **Hot things glow.** Every cell above about 500 °C glows: dark red, then orange, then yellow, then white. The glow comes from the temperature, so it works for every material automatically.
- Light: lava, fire, molten metal and crystals give light. Underground areas are dark. The robot has a lamp. The sky lights the surface.
- Bloom on bright light.
- Heat shimmer above very hot areas.
- Smoke and gas are partly transparent.
- Background: a far rock wall for each layer, with parallax. Sky with day and night on the surface.

### 20.3 Animation

- Machines move when they work (pistons, wheels, flames, glow) and stop when they stop.
- Belt surfaces move.
- Pipes show the flow direction in the back-layer view.
- When the robot digs, broken cells fly to the tool.
- Explosions: a flash, flying cells, smoke, a small screen shake.
- The camera follows the robot smoothly.

### 20.4 Sound (a later milestone)

Fire, water, steam, machine loops, explosions, digging. Each sound comes from its place in the world.

## 21. Scope of the first playable version

The first playable version has:

- The surface and upper stone layers, with the temperate, desert and tundra biomes.
- Tier 0 and Tier 1. The version ends when the player completes milestone stage 2.
- About 60 materials and 60 reactions.
- About 30 buildings, including 3 room machines (kiln, coke oven, blast furnace).
- Heat, fire, fluids, gases and explosions.
- Belts, hoppers, chutes, sorters, arms, pipes, pumps, valves and simple signals.
- Construction: ghosts, drag, rotate, pick, undo, copy and paste. The blueprint library can come later.
- Recipe browser, tech tree, machine panels, alerts, statistics.
- Save and load.

Later versions add Tiers 2 to 5, the deeper layers, drones, the blueprint library, sound and better art.

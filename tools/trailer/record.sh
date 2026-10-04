#!/usr/bin/env bash
# Records the shots of the trailer into out/trailer/shots (see tools/trailer/make.sh).
# Each shot is one `timtech --record` run: a scene, a camera move and scripted events.
set -euo pipefail
B=./target/fast/timtech
OUT=${OUT:-out/trailer/shots}
mkdir -p "$OUT"
V="--size 1920x1080"
shot() { local name=$1; shift; "$B" --record "$OUT/$name.mp4" $V "$@" | tail -1; }

# The surface of the generated world, from the left to the start.
shot 01_world --no-ui --seconds 4.5 --zoom 3 --zoom-to 3.4 --center -520,1000 --pan 70,0
# Sand and water pour onto the ground.
shot 02_pour --no-ui --seconds 4 --zoom 6 --center 0,1000 --pour sand,-40,-75,2 --pour water,35,-75,2,0.5 --pour sand,-10,-75,1,1.5
# Oil rains on the trees left of the start, then it burns.
shot 03_fire --no-ui --seconds 4 --speed 2 --zoom 6 --center -200,998 \
  --pour oil,-27,-40,2,0,1 --pour oil,27,-40,2,0,1 --pour fire,0,10,3,1.4,1.6 --pour fire,-40,12,3,1.4,1.6
# Explosions in the ground.
shot 04_boom --no-ui --seconds 3.5 --zoom 5 --center 0,1012 \
  --boom -70,20,0.3,160,900 --boom 10,24,0.9,190,1000 --boom 80,16,1.6,220,1200
# Lava pours into the lake left of the start: the water boils.
shot 05_lava --no-ui --seconds 4 --speed 2 --zoom 5 --zoom-to 5.5 --center -700,1015 --pour lava,0,-60,3,0 --pour lava,60,-60,2,0.8
# The robot runs, flies with the jetpack, lands and digs.
shot 06_robot --mode normal --no-hud --seconds 4.5 --zoom 8 --follow --drive "right:0.5,dig/14/12:1.6,right+jump:1.2,right:1.2"
# Machines with real fire and heat.
shot 07_kiln --ui-state kiln --no-hud --seconds 3.5 --zoom 9 --zoom-to 10.5 --center 60,1006
shot 08_smelter --ui-state smelter --no-hud --seconds 3.5 --zoom 11 --zoom-to 12.5 --center 77,1010
# Tier 1: a steam drill, arms, a steam furnace, a mold and an assembler.
shot 09_automation --ui-state automation --no-hud --seconds 4 --zoom 7 --center 95,1003 --pan 10,0
# The research window over the game.
shot 10_research --ui-state research --seconds 3 --ui-scale 1.0 --zoom 6 --zoom-to 6.5
# The cave under the start: glow moss and lamps.
shot 11_cave --ui-state cave --no-hud --seconds 4.5 --zoom 6 --follow --drive "left:0.3,right+jump:0.7,right:1.5,right+jump:0.6,right:1.4"
# A crystal geode deep in the rock.
shot 12_geode --no-ui --seconds 3.5 --zoom 9 --zoom-to 13 --center 985,4909
# The lava chamber, for the title.
shot 13_title --no-ui --seconds 6 --zoom 3 --zoom-to 4 --center 158,4262

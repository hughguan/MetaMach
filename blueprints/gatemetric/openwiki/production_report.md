# GateMetric - Production Report
#
# Prior Offboard smelting artifact (Feature-Spec §2.5). On the next Onboard the
# Daemon parses the bullet incidents below and injects them as
# `## Previous Incidents` few-shot examples into this blueprint's Agent System
# Prompt (experience inheritance, Test-Spec UTC-05-05).

## Compile Error History

- `MPU6050.cpp:42` undefined reference to `Wire.begin()` - add `#include <Wire.h>`.
- ESP32 linker: `.text` overflow by 4KB; drop `-O2` to `-Os`.

## Pin Conflict Details

- PIN_CONFLICT_MARKER_21: GPIO 21 I2C conflict with onboard LED; use GPIO 22.
- ESP32 timer collision on core 1; pin the filter task to core 0.

## Tool Guard Interception Logs

- `rm -rf build/` blocked (require_approval) - use `make clean` instead.
- `esptool.py erase_flash` blocked - require_approval HITL card.

## Successful Patches Applied

- I2C bus speed lowered to 100kHz for MPU6050 stability.
- DMP FIFO rate pinned to 200Hz to drop samples under vibration.

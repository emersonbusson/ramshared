FINDING_ONLY: Safe modification of scripts/safety/install.sh is impossible.
Evidence: The script is intentionally retained as an explicit disabled-staging boundary.
It contains the following comments:
# Legacy installer retained only as an explicit disabled-staging boundary.
# It must not write system paths, load modules, reload a manager, or arrange
# boot/service activation. A future attended transaction needs its own sealed
# implementation and approval; it is deliberately not provided here.
Any modification to enable version comparison and clean upgrades would violate its primary objective of being explicitly disabled and refusing installation.

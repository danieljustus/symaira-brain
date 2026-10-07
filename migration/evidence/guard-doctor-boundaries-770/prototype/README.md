# Author prototype receipts, not final acceptance

Baseline inventory and50 original nondeterminism observations are immutable in
baseline-8e3. Corrected dirty-source observations are distinct and do not replace
those originals. Twelve actual inline grammar/escape cases also match Go.
The first additive mutant wrapper hit a Python regex replacement escape error;
its actual runner/failure are retained. A corrected lambda replacement permits
the intended mutation and three actual mutants are rejected. The first strict
lint run rejected a helper after a test module; the helper was moved, no lint
suppression or assertion weakening. Focused170 tests and corrected strict gate
pass. Final clean-source-bound gates are separate and still require review.

"""
e3_analysis_fixtures.py — hermetic fixture builders for test_e3_analysis.py.

Not a test module (no test_ prefix, so no runner collects it). It builds the
small, self-consistent inputs the E3 analysis suite drives: a mini Protocol, a
fused-memory-shaped task snapshot, a temp runs.db carrying the live column
names, and a temp git repo with fixed committer dates. Everything it writes
goes under a caller-supplied temp directory.
"""

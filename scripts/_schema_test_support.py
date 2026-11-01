"""
_schema_test_support.py — a minimal, stdlib-only JSON-schema checker for the
schemas scripts/prd-capability-check.py and scripts/prd-decompose-verify.py
publish (`--print-schema` / `print-schema`).

It understands exactly the keywords those schemas use and raises ValueError on
any other, so a schema that adopts a new keyword fails loudly here instead of
validating vacuously.  It imports neither checker and defines no TestCase, so
the test_pcc_*.py and test_prd_decompose_verify*.py partitions can both use it
without loading either checker a second time.
"""

import json

_TYPE_CHECKS = {
    "object": lambda v: isinstance(v, dict),
    "array": lambda v: isinstance(v, list),
    "string": lambda v: isinstance(v, str),
    "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
    "boolean": lambda v: isinstance(v, bool),
    "null": lambda v: v is None,
}

_SUBSCHEMA_KEYWORDS = ("not", "if", "then", "items")

KEYWORDS = frozenset({
    "type", "properties", "required", "items", "enum", "const",
    "not", "if", "then", "oneOf", "minLength",
})


def _canonical(value):
    """A JSON spelling that keeps true and 1 apart, for equality and uniqueness."""
    return json.dumps(value, sort_keys=True)


def _refuse_unknown_keywords(schema, path):
    if not isinstance(schema, dict):
        raise ValueError(f"{path}: a schema must be an object, got {schema!r}")
    unknown = sorted(set(schema) - KEYWORDS)
    if unknown:
        raise ValueError(f"{path}: unsupported schema keyword(s) {unknown}")


def _type_errors(instance, expected, path):
    names = [expected] if isinstance(expected, str) else list(expected)
    for name in names:
        if name not in _TYPE_CHECKS:
            raise ValueError(f"{path}: unsupported type name {name!r}")
    if any(_TYPE_CHECKS[name](instance) for name in names):
        return []
    return [f"{path}: {instance!r} is not of type {expected!r}"]


def _object_errors(instance, schema, path):
    errors = []
    for key in schema.get("required", []):
        if key not in instance:
            errors.append(f"{path}: missing required property {key!r}")
    for key, subschema in schema.get("properties", {}).items():
        if key in instance:
            errors += schema_errors(instance[key], subschema, f"{path}.{key}")
    return errors


def schema_errors(instance, schema, path="$"):
    """Every way `instance` violates `schema`, one message each; [] when valid."""
    _refuse_unknown_keywords(schema, path)
    errors = []
    if "type" in schema:
        errors += _type_errors(instance, schema["type"], path)
    if "enum" in schema and _canonical(instance) not in map(_canonical, schema["enum"]):
        errors.append(f"{path}: {instance!r} is not one of {schema['enum']!r}")
    if "const" in schema and _canonical(instance) != _canonical(schema["const"]):
        errors.append(f"{path}: {instance!r} is not {schema['const']!r}")
    if isinstance(instance, str) and len(instance) < schema.get("minLength", 0):
        errors.append(f"{path}: {instance!r} is shorter than {schema['minLength']}")
    if isinstance(instance, dict):
        errors += _object_errors(instance, schema, path)
    if isinstance(instance, list) and "items" in schema:
        for index, item in enumerate(instance):
            errors += schema_errors(item, schema["items"], f"{path}[{index}]")
    if "not" in schema and not schema_errors(instance, schema["not"], path):
        errors.append(f"{path}: {instance!r} matches a schema it must not match")
    if "if" in schema and not schema_errors(instance, schema["if"], path):
        errors += schema_errors(instance, schema.get("then", {}), path)
    if "oneOf" in schema:
        matched = sum(not schema_errors(instance, option, path) for option in schema["oneOf"])
        if matched != 1:
            errors.append(f"{path}: {instance!r} matches {matched} oneOf options, not 1")
    return errors


def _subschemas(schema, path):
    """(path, subschema) for the schema itself and every schema nested in it."""
    yield path, schema
    if not isinstance(schema, dict):
        return
    for key, subschema in schema.get("properties", {}).items():
        yield from _subschemas(subschema, f"{path}.properties.{key}")
    for keyword in _SUBSCHEMA_KEYWORDS:
        if keyword in schema:
            yield from _subschemas(schema[keyword], f"{path}.{keyword}")
    for index, option in enumerate(schema.get("oneOf", [])):
        yield from _subschemas(option, f"{path}.oneOf[{index}]")


def assert_well_formed(testcase, schema):
    """Assert every subschema uses known keywords, requires only properties it
    declares, and carries only non-empty enums of distinct values."""
    for path, subschema in _subschemas(schema, "$"):
        with testcase.subTest(subschema=path):
            testcase.assertIsInstance(subschema, dict)
            testcase.assertLessEqual(set(subschema), KEYWORDS)
            testcase.assertLessEqual(
                set(subschema.get("required", [])), set(subschema.get("properties", {}))
            )
            if "enum" in subschema:
                enum = subschema["enum"]
                testcase.assertIsInstance(enum, list)
                testcase.assertTrue(enum, "an enum admits nothing when empty")
                spellings = [_canonical(value) for value in enum]
                testcase.assertEqual(len(spellings), len(set(spellings)), enum)

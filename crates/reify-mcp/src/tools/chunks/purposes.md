# Purpose Declarations

Purposes are named, parameterized declaration kinds that control what constraints and objectives are active. They express requirements over the shape and state of a design.

## Syntax

```
purpose manufacturing_ready(subject : Structure) {
    constraint forall p in subject.geometric_params: determined(p)
    constraint forall p in subject.material_params: determined(p)
    minimize subject.cost
}
```

## Activation

When activated, a purpose's constraints and outputs are present in the evaluation graph; when deactivated, they are absent. The checking/solving/proposing mode is determined by input determinacy state.

## Entity References

Purpose parameters are entity references, not values. Type annotation is an entity-kind selector: `Structure`, `Occurrence`, `Constraint`, or `Field`.

Entity references provide reflective access:

| Member | Returns | Meaning |
|--------|---------|---------|
| `.params` | `List<ParamRef>` | All param declarations |
| `.geometric_params` | `List<ParamRef>` | Params with geometric dimensions |
| `.material_params` | `List<ParamRef>` | Params with material properties |
| `.sub_entities` | `List<EntityRef>` | All sub declarations |
| `.ports` | `List<PortRef>` | All port declarations |
| `.constraints` | `List<ConstraintRef>` | All constraint declarations |

## Standard Library Purposes

- `design_review` — checks all parameters are at least constrained
- `simulation_ready` — checks geometry and material are fully determined
- `manufacturing_ready` — checks everything needed for manufacturing

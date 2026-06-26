# position vs. velocity modes
- we can either set the desired target for a target position (cursor pointing), or target the velocity (turn right, turret accelerates up to max speed) without a target

# rig: RON-authored contract & composable primitives (PROVISIONAL, deferred)
- design sketch parked in `.agents/docs/design/rig-ron-sot-and-composability.md` — idea, not a decision; deferred behind the vertical slice. Move the name→behaviour join out of code into RON; axis-as-node; headless "RON matches model" test. Revisit only after the slice (and only once a deliberately-weird 2nd tank exists to find the real primitives).
## Summary

<!-- What changes, and why. -->

## Testing

- [ ] `uv run poe check` passes (ruff, format, mypy, pytest)
- [ ] Ran it, or said below why that wasn't possible

CI has no OpenVINO and no NPU. Say which of these applies:

- [ ] Verified on the NPU
- [ ] Verified on GPU / CPU (say which)
- [ ] Logic only; nothing audible changed

## Checklist

- [ ] Comments explain *why*, not what, and any measured claim carries a date
- [ ] README / CONTRIBUTING updated if behaviour or setup changed
- [ ] If the model changed: `scripts/update_pins.py` re-run and the diff read
- [ ] No secrets, tokens or machine-specific paths committed

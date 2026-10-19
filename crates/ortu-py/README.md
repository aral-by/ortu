# ortu for Python

Python binding for [ortu](https://github.com/aral-by/ortu): PII detection, masking and encryption for Turkish speech and LLM applications. The binding is a thin wrapper; all logic lives in the Rust core.

```python
import ortu

engine = ortu.Engine(key="00" * 32)          # or keys from ORTU_TOKEN_KEY
session = engine.session(call_id="call-1")

caller = session.register("phone", "0532 123 45 67")       # token for the system prompt
masked = session.push({"speaker": "bot", "text": "TC kimlik numaranızı alabilir miyim?"})
masked = session.push({"speaker": "caller", "text": "on sıfır sıfır sıfır sıfır"})
masked = session.push({"speaker": "caller", "text": "sıfır sıfır yüz kırk altı"})
token = masked["entities"][0]["replacement"]               # "[TCKN:...]", what the LLM sees

args = session.guard_tool_args("query_debt", {"tckn": token})   # {"tckn": "10000000146"}
result = session.mask_tool_result("query_debt", {"name": "Ahmet Yılmaz", "debt": 1250})
spoken = session.rehydrate("Sayın " + result["name"])           # "Sayın Ahmet Yılmaz"
post = session.finish()                                          # persistent transcript
```

A value the LLM invents or corrupts raises `ortu.GuardError` instead of reaching a tool.

Development build (from the repository root): `cargo xtask py` builds the extension into `.venv` with maturin and runs the Python tests.

"""A voice-agent loop with ortu between STT, the LLM, a tool and TTS.

The LLM is scripted so that the example runs without a provider or network
access; replace `llm()` with a call to any LLM client. Everything printed under
"LLM sees" is what would leave for the provider: tokens only. All values are
synthetic.

Run from the repository root after `cargo xtask py`:

    .venv/Scripts/python examples/voice_agent.py      (Windows)
    .venv/bin/python examples/voice_agent.py          (Linux, macOS)
"""

import json
import os
import re

import ortu

# A development key; deployments use ORTU_TOKEN_KEY (or ORTU_TOKEN_KEY_<TENANT>).
KEY = os.environ.get("ORTU_TOKEN_KEY", "5e" * 32)

# Final STT turns of one call, as the STT would deliver them.
CALL = [
    {"speaker": "caller", "text": "merhaba borcumu öğrenmek istiyorum"},
    {"speaker": "caller", "text": "TC'm on sıfır sıfır sıfır sıfır"},
    {"speaker": "caller", "text": "sıfır sıfır yüz kırk altı"},
]


def query_debt(tckn):
    """An internal system: it needs the real TCKN."""
    assert tckn == "10000000146", "the tool must receive the real value"
    return {"name": "Ahmet Yılmaz", "tckn": tckn, "debt_try": 1250, "due": "2026-11-15"}


def llm(messages):
    """A scripted stand-in for an LLM: asks for the debt once a TCKN token
    appears, then answers with the tokens it was given."""
    last = messages[-1]
    if last["role"] == "tool":
        result = json.loads(last["content"])
        return {
            "content": f"Sayın {result['name']}, {result['debt_try']} TL borcunuz var; "
            f"kimlik numaranız {result['tckn']}."
        }
    tokens = re.findall(r"\[TCKN:[0-9a-f]{8}\]", last["content"])
    if tokens:
        return {"tool_call": {"name": "query_debt", "arguments": {"tckn": tokens[-1]}}}
    return {"content": "Kimlik numaranızı alabilir miyim?"}


def main():
    engine = ortu.Engine(key=KEY)
    session = engine.session(call_id="demo-call-1", tenant_id="demo")
    caller_id = session.register("phone", "0532 123 45 67")
    messages = [{"role": "system", "content": f"Arayan numara: {caller_id}"}]
    print(f"system prompt   {messages[0]['content']}")

    for turn in CALL:
        masked = session.push(turn)
        print(f"\ncaller said     {turn['text']}")
        print(f"LLM sees        {masked['text']}")
        messages.append({"role": "user", "content": masked["text"]})
        reply = llm(messages)

        if "tool_call" in reply:
            call = reply["tool_call"]
            print(f"LLM calls       {call['name']}({json.dumps(call['arguments'], ensure_ascii=False)})")
            args = session.guard_tool_args(call["name"], call["arguments"])
            result = query_debt(**args)
            masked_result = session.mask_tool_result(call["name"], result)
            print(f"tool returns    {json.dumps(masked_result, ensure_ascii=False)}  (as the LLM sees it)")
            messages.append({"role": "tool", "content": json.dumps(masked_result, ensure_ascii=False)})
            reply = llm(messages)

        # Stream the reply to TTS through the rehydrator, as an LLM would send it.
        rehydrator = session.rehydrator()
        content = reply["content"]
        spoken = "".join(rehydrator.feed(content[i : i + 7]) for i in range(0, len(content), 7))
        spoken += rehydrator.finish()
        print(f"LLM replies     {content}")
        print(f"TTS speaks      {spoken}")
        # What the bot said is part of the call: its question tells ortu what
        # the caller's next turn holds, and it belongs in the transcript.
        session.push({"speaker": "bot", "text": spoken})
        messages.append({"role": "assistant", "content": content})

    # An LLM that invents a number never reaches the tool.
    try:
        session.guard_tool_args("query_debt", {"tckn": "10000000164"})
    except ortu.GuardError as error:
        print(f"\ninvented value  rejected: {error.reason} ({error.kind})")

    post = session.finish()
    print("\npersistent transcript (post-call):")
    for turn in post["turns"]:
        print(f"  {turn['speaker']:<7} {turn['text']}")


if __name__ == "__main__":
    main()

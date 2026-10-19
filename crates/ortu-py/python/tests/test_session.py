"""End-to-end tests of the Python binding (M3 acceptance: the voice-agent loop
from Python, docs/systemdesign.md sections 13.3 and 17)."""

import unittest

import ortu

KEY = "2a" * 32


def turns(*pairs):
    return [{"speaker": speaker, "text": text} for speaker, text in pairs]


CALL = turns(
    ("bot", "TC kimlik numaranızı alabilir miyim?"),
    ("caller", "on sıfır sıfır sıfır sıfır"),
    ("bot", "evet"),
    ("caller", "sıfır sıfır yüz kırk altı"),
)


class VoiceAgentLoop(unittest.TestCase):
    def setUp(self):
        self.engine = ortu.Engine(key=KEY)
        self.session = self.engine.session("call-1", "tenant")

    def test_a_split_tckn_reaches_the_tool_as_a_real_value(self):
        texts = [self.session.push(turn)["text"] for turn in CALL]
        self.assertTrue(texts[1].startswith("[TCKN_INCOMPLETE:"), texts[1])
        self.assertEqual(texts[2], "evet")
        self.assertRegex(texts[3], r"^\[TCKN:[0-9a-f]{8}\]$")

        released = self.session.guard_tool_args("query_debt", {"tckn": texts[3]})
        self.assertEqual(released, {"tckn": "10000000146"})

    def test_an_invented_value_raises_guard_error(self):
        with self.assertRaises(ortu.GuardError) as caught:
            self.session.guard_tool_args("query_debt", {"tckn": "10000000164"})
        self.assertEqual(caught.exception.reason, "unknown_value")
        self.assertEqual(caught.exception.kind, "tckn")
        self.assertIsInstance(caught.exception, ortu.OrtuError)

        with self.assertRaises(ortu.GuardError) as caught:
            self.session.guard_tool_args("query_debt", {"tckn": "[TCKN:00000000]"})
        self.assertEqual(caught.exception.reason, "unknown_token")

    def test_tool_results_and_read_back(self):
        result = self.session.mask_tool_result(
            "query_debt", {"name": "Ahmet Yılmaz", "debt": 1250, "tags": ["a"]}
        )
        self.assertTrue(result["name"].startswith("[NAME:"), result)
        self.assertEqual(result["debt"], 1250)
        self.assertEqual(result["tags"], ["a"])

        phone = self.session.register("phone", "0532 123 45 67")
        reply = f"Sayın {result['name']}, numaranız {phone}."
        self.assertEqual(
            self.session.rehydrate(reply), "Sayın Ahmet Yılmaz, numaranız *********4567."
        )

    def test_streaming_rehydration(self):
        name = self.session.register("full_name", "Ayşe Kaya")
        rehydrator = self.session.rehydrator()
        out = rehydrator.feed("Merhaba " + name[:4])
        out += rehydrator.feed(name[4:] + "!")
        out += rehydrator.finish()
        self.assertEqual(out, "Merhaba Ayşe Kaya!")

    def test_finish_returns_the_persistent_transcript_and_closes_the_session(self):
        for turn in CALL:
            self.session.push(turn)
        post = self.session.finish()
        self.assertEqual([e["kind"] for e in post["entities"]], ["tckn"])
        entity = post["entities"][0]
        self.assertTrue(entity["cross_turn"])
        self.assertEqual([p["turn"] for p in entity["parts"]], [1, 3])
        self.assertEqual(post, self.engine.redact(CALL, "call-1", "tenant"))
        with self.assertRaises(ortu.OrtuError):
            self.session.push({"speaker": "caller", "text": "merhaba"})


class Configuration(unittest.TestCase):
    def test_policy_and_errors(self):
        engine = ortu.Engine('[policy]\nmode = "balanced"\n', key=KEY)
        masked = engine.session("c").push({"speaker": "caller", "text": "TC 10000000146"})
        self.assertRegex(masked["text"], r"^TC \[TCKN:[0-9a-f]{8}\]$")
        self.assertEqual(masked["entities"][0]["validator"], "passed")

        with self.assertRaises(ValueError):
            ortu.Engine("[policy]\nunknown = 1\n", key=KEY)
        with self.assertRaises(ValueError):
            ortu.Engine(key="not hex")
        with self.assertRaises(ValueError):
            engine.session("c").register("shoe_size", "42")

    def test_word_level_turns_keep_their_timing(self):
        engine = ortu.Engine(key=KEY)
        words = [
            {"text": "TC'm", "start_ms": 0, "end_ms": 300, "confidence": 0.9},
            {"text": "10000000146", "start_ms": 300, "end_ms": 1500, "confidence": 0.8},
        ]
        masked = engine.session("c").push({"speaker": "caller", "words": words})
        self.assertRegex(masked["text"], r"^TC'm \[TCKN:[0-9a-f]{8}\]$")
        self.assertEqual(masked["entities"][0]["parts"], [{"turn": 0, "words": [1, 2]}])


if __name__ == "__main__":
    unittest.main()

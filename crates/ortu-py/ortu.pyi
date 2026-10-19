"""Type stubs for the `ortu` native module (docs/systemdesign.md, section 13.3)."""

from typing import Any, Literal, Optional, Sequence, TypedDict, Union

Speaker = Literal["caller", "agent", "bot", "unknown"]
Kind = Literal[
    "tckn",
    "vkn",
    "iban",
    "phone",
    "credit_card",
    "email",
    "person_name",
    "person_surname",
    "full_name",
    "address",
    "plate",
    "birth_date",
    "passport_no",
]
Validator = Literal["passed", "not_applicable", "too_short", "too_long", "checksum", "format"]

class WordDict(TypedDict, total=False):
    text: str
    start_ms: int
    end_ms: int
    confidence: float

class TurnDict(TypedDict, total=False):
    speaker: Speaker
    text: str
    words: list[WordDict]
    index: int
    start_ms: int
    end_ms: int

class Part(TypedDict):
    turn: int
    words: list[int]

class Span(TypedDict):
    turn_start: int
    byte_start: int
    turn_end: int
    byte_end: int

class Entity(TypedDict):
    id: int
    kind: Kind
    parts: list[Part]
    span: Span
    score: float
    review: bool
    validator: Validator
    replacement: str
    detector: str
    cross_turn: bool

class MaskedTurn(TypedDict):
    index: int
    speaker: Speaker
    text: str
    entities: list[Entity]

class RedactedTurn(TypedDict):
    index: int
    speaker: Speaker
    text: str

class PostCallResult(TypedDict):
    turns: list[RedactedTurn]
    entities: list[Entity]

Json = Union[None, bool, int, float, str, list[Any], dict[str, Any]]

class OrtuError(Exception):
    """The engine or a session could not do what was asked; no text was released."""

class GuardError(OrtuError):
    """A value leaving the LLM was refused."""

    reason: Literal[
        "unknown_token", "ambiguous_token", "invalid_value", "unknown_value", "audit_failed"
    ]
    kind: Optional[Kind]

class Engine:
    def __init__(
        self,
        policy: Optional[str] = None,
        *,
        key: Optional[str] = None,
        weights: Optional[str] = None,
        read_back: Optional[Sequence[tuple[Kind, Literal["full", "partial"]]]] = None,
        accept_invalid: Optional[Sequence[tuple[str, Kind]]] = None,
        max_open_turns: int = 3,
        agent_names: Optional[Sequence[str]] = None,
    ) -> None: ...
    @staticmethod
    def from_config(path: str, *, key: Optional[str] = None) -> "Engine": ...
    def session(self, call_id: str, tenant_id: str = "default") -> "Session": ...
    def redact(
        self, turns: Sequence[TurnDict], call_id: str, tenant_id: str = "default"
    ) -> PostCallResult: ...

class Session:
    def push(self, turn: TurnDict) -> MaskedTurn: ...
    def register(self, kind: Kind, value: str) -> str: ...
    def guard_tool_args(self, tool: str, args: Json) -> Json: ...
    def mask_tool_result(self, tool: str, result: Json) -> Json: ...
    def rehydrate(self, text: str) -> str: ...
    def rehydrator(self) -> "Rehydrator": ...
    def finish(self) -> PostCallResult: ...

class Rehydrator:
    def feed(self, chunk: str) -> str: ...
    def finish(self) -> str: ...

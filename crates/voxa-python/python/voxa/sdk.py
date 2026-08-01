"""High-level Python Node development API."""

from dataclasses import dataclass
from typing import Any, List, Optional, Union, cast

from ._native import (
    AudioFrame,
    ByteFrame,
    EventFrame,
    Frame,
    PythonNodeExecutionDomain,
    SignalFrame,
    TextFrame,
    VideoFrame,
)

FrameValue = Union[
    Frame,
    AudioFrame,
    VideoFrame,
    TextFrame,
    ByteFrame,
    SignalFrame,
    EventFrame,
]
NodeOutput = Optional[Union[FrameValue, List[FrameValue]]]


class TransformNode:
    """Base class for a Voxa Node.

    Any callback may be overridden with either ``def`` or ``async def``. The
    callbacks run serially on the Node's dedicated execution thread.
    """

    def on_prepare(self) -> None:
        pass

    def on_process(self, frame: FrameValue) -> NodeOutput:
        return frame

    def on_signal(self, signal: SignalFrame) -> None:
        pass

    def on_event(self, event: EventFrame) -> None:
        pass

    def on_finish(self) -> None:
        pass

    def on_abort(self, reason: str) -> None:
        pass


@dataclass(frozen=True)
class NodeOptions:
    """Bounded execution settings for one Python Node."""

    inbox_capacity: int = 16
    completion_capacity: int = 16
    max_in_flight: int = 1
    call_deadline_ms: int = 10_000
    shutdown_deadline_ms: int = 5_000
    ordering: str = "strict"
    isolation: str = "in_process"


class NodeRunner:
    """Owns one Python Node execution domain and its lifecycle."""

    def __init__(self, node: Any, options: Optional[NodeOptions] = None) -> None:
        self.node = node
        self.options = options or NodeOptions()
        self._domain = PythonNodeExecutionDomain(
            node,
            inbox_capacity=self.options.inbox_capacity,
            completion_capacity=self.options.completion_capacity,
            max_in_flight=self.options.max_in_flight,
            call_deadline_ms=self.options.call_deadline_ms,
            shutdown_deadline_ms=self.options.shutdown_deadline_ms,
            ordering=self.options.ordering,
            isolation=self.options.isolation,
        )
        self._started = False
        self._finished = False

    @property
    def domain(self) -> PythonNodeExecutionDomain:
        """Low-level domain, useful for EventBus subscriptions."""
        return self._domain

    @property
    def is_closed(self) -> bool:
        return self._domain.is_closed

    def start(self) -> "NodeRunner":
        if self.is_closed:
            raise RuntimeError("NodeRunner is closed")
        if not self._started:
            self._domain.prepare()
            self._started = True
        return self

    def process(self, frame: FrameValue) -> List[FrameValue]:
        """Process one owned frame, starting the Node on first use."""
        self.start()
        if self._finished:
            raise RuntimeError("NodeRunner is finished")
        return cast(List[FrameValue], self._domain.process(frame))

    def signal(self, signal: SignalFrame) -> None:
        self.start()
        self._domain.signal(signal)

    def event(self, event: EventFrame) -> None:
        self.start()
        self._domain.event(event)

    def finish(self) -> bool:
        """Run ``on_finish`` exactly once. The domain remains closable."""
        if self.is_closed or self._finished:
            return False
        self.start()
        self._domain.finish()
        self._finished = True
        return True

    def abort(self, reason: str) -> bool:
        """Run ``on_abort`` exactly once with an owned diagnostic string."""
        if self.is_closed or self._finished:
            return False
        self.start()
        self._domain.abort(reason)
        self._finished = True
        return True

    def close(self) -> bool:
        """Close the execution domain. Safe to call more than once."""
        return self._domain.close()

    def __enter__(self) -> "NodeRunner":
        return self.start()

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        try:
            if exc_type is None:
                self.finish()
            else:
                try:
                    self.abort(str(exc))
                except Exception:
                    # Preserve the exception that caused the context to abort.
                    pass
        finally:
            self.close()

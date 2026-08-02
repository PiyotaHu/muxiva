# Qwen Provider

The Qwen provider is implemented entirely in Python under `python/nodes`:

- `qwen_realtime`: end-to-end audio realtime model.
- `qwen_asr_realtime`: streaming speech recognition.
- `qwen_llm_stream`: streaming language model.
- `qwen_tts_realtime`: streaming speech synthesis.

Install dependencies with `python3 -m pip install -r python/requirements.txt`
and run protocol tests with
`python3 -m unittest discover -s providers/algorithm/qwen/python/tests -v`.

For the supported region, API Key, Workspace ID, and no-SDK-download setup,
read the [Qwen Provider guide](../../../docs/providers/qwen.md).

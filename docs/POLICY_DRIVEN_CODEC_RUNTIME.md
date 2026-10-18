# Policy-driven codec execution

`policy_runtime::execute_policy` executes the existing `FormatMatrix` candidate generator under a validated `CodecPolicy`.

The caller supplies `count_tokens(&str) -> u64`; production callers should pass the exact tokenizer used by the target model. Counts are taken over the original input and the complete encoded wire, including headers and dictionaries. The runtime rejects unknown wire markers, filters candidates whose codec families are disabled, disallows multi-codec chains unless composition is enabled, and checks reconstruction before a candidate can win. The lowest-token eligible candidate is returned, with wire length as a deterministic tie-breaker.

The result reports the chosen wire, codec path, candidate counts, input token count, elapsed microseconds, and a `FitnessMeasurement` suitable for the policy tuner.

Important boundaries:
- This stage does not select or activate a stored policy automatically; the host loads a policy and explicitly calls the runtime.
- If no candidate passes the policy and reconstruction checks, `selected` is `None`; callers should keep the original input.
- The tokenizer callback is deliberately injected. Tests use simple counters only to test plumbing, not to claim model-token savings.
- File kind remains policy metadata for selecting the caller's policy. A production host should classify input before loading the corresponding stored policy.

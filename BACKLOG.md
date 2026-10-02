1. Fix "instruction leakage"
https://github.com/openai/codex/issues/17224

Sample:
I discuss with AI that I don't want the enum to be named using uppercase and underscore, but instead i want it to be lowercase and underscore, because it's matching the company convention more. The AI then proceed writing doc comment on top of the enum like this: /** Name is in lowercase (not uppercase based on common convention) is expected to match the company's convention. **/ enum Sample { main_branch, second_branch }

Possible prompt:
```
Don't leak conversational context into the generated artifact. Use discussion context to make implementation decisions, but don't encode that context as comments or documentation unless it is materially relevant to future maintainers. Avoid meta-comments explaining why you followed my instruction.
```

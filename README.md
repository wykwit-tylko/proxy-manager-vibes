Subjective snapshot of LLMs Rust proficiency levels.

## Setup

The task prompt was passed straight from the [TASK.md](/TASK.md) file.

> Given the proxy-manager.py CLI tool create a complete re-implementation in Rust with an additional TUI.

You can see the Python reference implementation in the [initial commit](https://github.com/wykwit-tylko/proxy-manager-vibes/commit/9339c484b15bda02e7051d4974037d09ff2945fe) (9339c48).

All models were run with [opencode](https://github.com/anomalyco/opencode) through [open-ralph-wiggum](https://github.com/Th0rgal/open-ralph-wiggum) without any additional MCP servers enabled.

```
ralph --max-iterations 5 -f TASK.md --model <provider/model>
```

## Models

In my order of preference...

| N | Lab         | Model             | Provider       | Coding Index | Agentic Index | Results | Rank |
|---|-------------|-------------------|----------------|--------------|---------------|---------|------|
| 1 | Z.AI        | GLM-4.7           | Z.AI           | 36 (#6)      | 55 (#4)       | Success | A+   |
| 2 | Minimax     | M2.1              | OpenCode Zen   | 33 (#8)      | 47 (#8)       | Success | C-   |
| 3 | Moonshot AI | Kimi K2.5         | Kimi Code      | 40 (#5)      | 59 (#2)       | Failure | F    |
| 4 | Moonshot AI | Kimi K2 Thinking  | Kimi Code      | 35 (#7)      | 48 (#7)       | Success | A    |
| 5 | OpenAI      | GPT-5.2           | Github Copilot | 49 (#1)      | 60 (#1)       | Success | S    |
| 6 | OpenAI      | GPT-5.2-Codex     | GitHub Copilot | 43 (#3)      | 57 (#3)       | Success | C    |
| 7 | Google      | Gemini 3 Pro      | Google         | 46 (#2)      | 52 (#5)       | Failure | F    |
| 8 | Google      | Gemini 3 Flash    | Google         | 43 (#4)      | 50 (#6)       | Success | B    |

## Results

### 1. GLM-4.7

Iterations to complete: **4**

Branch: [glm-4.7](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/glm-4.7)

Starting off with documentation - I really like how nicely this model reports progress. We also got a pretty README.md file. This is the only model that generated a full Cargo.toml with description, default license, accurate keywords, and categories. It's also the only model that opted for creating the TUI as a separate binary instead of a subcommand. The structure is laid out nicely with very good separation of concerns for each module. Pretty much all the modules contain "managers" implemented in an object-oriented style. Dependencies between modules are passed explicittly by calling the constructor with an instance of another manager or client. In some places we can see 'use' imports and full namespace qualifiers inconsistently mixed up, which is a little annoying. We are missing unit tests, except for a few in the config module. I don't spot anything particularly unexpected in the generated code. If I were dropped in this codebase I could easily find my way around and make changes. In this case the TUI is a failure. The way it's done is a bunch of menus to execute CLI subcommands. No overview of the status, no container lists, overall a pretty weak attempt. Disregarding the TUI, the model gave me exactly what I would expect. It may have difficulties with Rust sometimes, but in the end the project works and feels good enough. It's worth mentioning that the original proxy-manager.py was also made with this model. This model is definitely underaprreciated and it quickly became my favorite once I tried using it more.

Subjective Rank: A+

### 2. Minimax M2.1

Iterations to complete: **3**

Branch: [minimax-m2.1](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/minimax-m2.1)

This model would not stop util it satisfied the prompt, but on the first run it marked the task as complete when it only had a scaffold in place with maybe a few stub methods. Eventually it fleshed them out and in the end it delivered a working project. What was not working however, was the TUI. That required an additional prompt to stop it from crashing on run time. Even though it stopped crashing after that, the TUI was clearly not laid out well and even though it technically "works" it's not functional at all. We got an OK kinda README generated after an additional prompt too.

This is the only model that opted to mark TUI as a feature, making it optional during compile time. The separation of modules is a little confusing, even though at first glance the modules it decided to go with seem to make a good structure. The problem is with a few things being miscategorized or mislabeled. The modules are implemented in a bag-of-functions pattern. That's a valid approach, and I'd use it often myself, but then here the naming of functions is inconsistent, so it's just confusing and doesn't make much sense in the end. We are also missing unit tests or we got tests that don't test anything. I definitely wouldn't want to work with this project implementation any further.

The way this model does things is mostly correct, but it just feels cheap. And it is - in many places it's even available for free. The results are not always fully complete or of the quality we would hope for. Still it is good enough and I'd say that for filling out basic functions FAST and CHEAP it's quite a good choice.

Subjective Rank: C-

### 3. Kimi K2.5

**FAILURE**

I've tried letting it run for more iterations and I've tried giving it more directed instructions, but at this time the model gets completely lost. It starts exploring and finishes the run before doing any implementation. It sends down invalid tool calls. Maybe something is wrong with my harness, maybe the issue is that this model is still too fresh, but it just doesn't work. It can emit some code, but it completely fails when working on projects. From the benchmarks and the original announcement this seems to be a good model for writing, planning, splitting work, and creating prompts for other agents. Unfortunately it miserably fails as an executor.

### 4. Kimi K2 Thinking

Iterations to complete: **1**

Branch: [kimi-k2](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/kimi-k2)

It came up with the best TUI. It completed the whole task in one iteration. This is pretty crazy by itself. We even got a few unit tests. There is a short, but well-structured README, although it mentions a hallucinated sub-command that is not implemented. There were some clippy warnings left over, but they didn't affect compilation, and got quickly fixed with a single additional prompt. This model sprinkles in a few comments from time to time. Looking at Cargo.toml we see our dependencies organized by category with comments telling us what they are there for. The project structure is OK. There's both a lib and main files, but the lib seems unneccessary in this case. All modules are put in their own directories and mostly implemented in a single mod file. In this case I would prefer a flat structure. The separation could be better, but this isn't too bad. Most important modules are implemented in the obejct-oriented fashion similar to how GLM-4.7 did it. I like the visual separation of steps in longer functions. This code reads really well. After the failure of the younger brother K2.5 it's a pleasant surprise to see that good of a result. Fells like this model knows how to write Rust.

Subjective Rank: A

### 5. GPT-5.2

Iterations to complete: **4**

Branch: [gpt-5.2](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/gpt-5.2)

This model needs no introductions. At the moment of writing this it's generally accepted as the best in every way and for a good reason. I also like it a lot for day to day work. The implementation it provided is very sophisticated (perhaps to it's detriment). We got the most complete test suite so far. We got a working TUI. We got very good separation and organization of modules. It even came up with the app module that unifies behaviour for the tool. Good patterns used throughout the project. Not much to add, this is just solid. It gets the job done very well.

Subjective Rank: S

### 6. GPT-5.2-Codex

Iterations to complete: **2**

Branch: [gpt-5.2-codex](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/gpt-5.2-codex)

This version is way worse than the previous attempt. This time we don't have a README or a nicely done progress report. The TUI starts up, but it doesn't have any actions implemented. There is no sophistication whatsoever. We are back to the bag-of-functions pattern, but at least the functions are grouped well this time. The project is kind of a mess. Looking up close at the code it is just repulsive - especially after seeing what regular GPT-5.2 can do and how well. It's clear this is a cheaper and faster model with a drastically different use-case. It does not work out well on our example. I'd rather use any of the open-weight models than rely on this one.

Subjective Rank: C

### 7. Gemini 3 Pro

**FAILURE**

The model would report completion after the first iteration, even though there was only a basic scaffold without any actual implementation. It was also the only model to create the project in a subdirectory instead of root of the repo. A lot of fluff, not much work. I'm surprised how it scores so high on the benchmarks.

### 8. Gemini 3 Flash

Iterations to complete: **1**

Branch: [gemini-3-flash](https://github.com/wykwit-tylko/proxy-manager-vibes/tree/gemini-3-flash)

Unlike with the bigger brother the completion status emitted after first iteration was valid. The model started off similarly to the previous one, by creating parts of the project in a subdirectory, but it quickly corrected itself. Much like Kimi K2 Thinking, it got surprisingly good results fast, but in this case speed seems to be the only unique advantage of the model. Progress tracking is OK, we don't have a README, the Cargo.toml is minimal. The separation of modules could be better - the only one proper single concern module is the config. TUI seems to give us more or less the overview of current status, but it's nothing special. In a few more prompts maybe you could steer this implementation attempt the right way, but left to itself, Gemini just doesn't deliver much. Perhaps if it didn't stop after the first iteration it could end up with a better result. This model is good at many things, but it's definitely not the best at coding.

Subjective Rank: B

## Conclusion

The open-weight models in 2026 are INSANE.


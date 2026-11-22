# LangBridge

模型使用中文和英文推理，成本并不相同。LangBridge 回答一个具体问题：这一次，用哪种语言思考更省，能省多少 Token。

它不猜测，也不套用固定规则。内置数据可以直接出结论；接上自己的 API Key，可以得到该模型专属的中英 Token 成本比，并把测量结果写回模型列表。

## 它解决什么

同一个问题，用中文思考还是用英文思考，会同时影响三件事：

- 推理阶段消耗多少 Token
- 回答质量是否下降
- 切换语言本身要付出多少注入开销

LangBridge 把这三件事拆开计算，最后只给一个结论：换，还是不换。如果净收益没有超过门槛，它会直接说明不建议换，而不是勉强给出一个省字。

## 核心能力

- **四层路由**：内容规则、模型特点、输入语言、成本计算依次判断，前一层命中后不再往下评估。
- **内置参考数据**：DeepSeek-R1、DeepSeek-V3、Qwen3-Max 三组离线证据，无需 Key，打开页面即可点击出结论。
- **API 真实测量**：支持 OpenAI 兼容接口，用自己的 Key 测出模型专属成本比。测量成功后，模型会自动进入左上角模型列表，并标注实测比值。
- **分步引导**：选服务商、填 Key、测试连接、开始测量，每一步都有当前状态与下一步提示。
- **注入方案**：给出可直接复制的语言约束提示，说明流式输出与多语言冲突时的处理规则。
- **本地代理与 CLI**：在本机提供 OpenAI 兼容入口，按模型注入约束、透传上游、后处理非流式输出。
- **隐私优先**：默认全部在本地计算，粘贴的内容不出浏览器。

## 使用流程

1. 在输入区粘贴问题，或点击示例与内置模型。
2. 页面在浏览器内提取输入语言、任务类型、是否含代码等信号。
3. 引擎完成四层路由，给出语言建议、净收益与门槛。
4. 需要精确数字时，在 API 测量区用你自己的 Key 实测该模型。
5. 在「怎么告诉模型换语言」中复制提示，放进你的调用。

## 快速开始

### Web

在线体验：<https://langbridge.traesite.cn>

本地构建需要 Rust 工具链、`wasm32-unknown-unknown` 目标与 Trunk：

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd crates/langbridge-web
trunk serve
```

### CLI

```bash
cargo run -p langbridge-cli -- --help

# 零 Key：用内置证据为三个模型写入本地画像
cargo run -p langbridge-cli -- setup

# 对一条输入做四层路由与净收益决策
cargo run -p langbridge-cli -- decide --model deepseek-reasoner \
  --text "请解释闭包在 JavaScript 中的作用"
```

需要模型实测时：

```bash
export LANGBRIDGE_API_KEY=...         # 也可用 --api-key
cargo run -p langbridge-cli -- calibrate --model deepseek-reasoner \
  --method api --base-url https://api.deepseek.com/v1
```

### 本地代理

```bash
cargo run -p langbridge-cli -- proxy --upstream https://api.deepseek.com/v1 \
  --port 8787 --model deepseek-reasoner
# OpenAI 兼容入口：http://127.0.0.1:8787/v1
```

Key 用 `--api-key` 或环境变量 `LANGBRIDGE_API_KEY` 传入；不传时透传请求头里的 `Authorization`。只想查看注入计划、不请求上游：

```bash
cargo run -p langbridge-cli -- proxy --dry-run --text "请分析远程办公对协作效率的影响。"
```

## 测量方法

同语义的中英文本各取 N = 120、240、480 三档，共 6 条观测。对每种语言拟合 `T = a + c·N`，固定的前导开销在差分中抵消，中英斜率之比就是成本比。

内置证据使用各模型公开 tokenizer 的真实计数：DeepSeek-R1 与 DeepSeek-V3 约为 0.89，Qwen3 约为 0.95。Qwen3-Max 未公开自身 tokenizer，使用同系列开源模型代算，页面会明确标注这一点。

测量结果会经过质量闸门：R²、线性度与置信度不达标时，不输出成本结论，只保留语言建议。

## 隐私与边界

- 默认本地计算：Web 在浏览器内完成，CLI 在本机完成。
- 不把粘贴的问题上传到任何服务。只有你主动做 API 测量或使用本地代理时才会出网，测量请求只包含内置校准语料。
- API Key 只留在内存：Web 刷新即清除，CLI 只存在当前进程，不写入画像文件。
- 成本结论只反映 Token 数量差异，不含单价、缓存折扣、汇率等计费规则，不等于账单金额。
- 浏览器直连第三方 API 会受 CORS 限制，被拦截时改用 CLI 校准或本地代理。

## 技术架构

纯 Rust 实现，工作区分三层：

| 目录 | 职责 |
| --- | --- |
| `crates/langbridge-core` | 四层路由、校准拟合、注入协议、净收益、输出守卫、画像校验与内置证据，纯计算，无 IO |
| `crates/langbridge-web` | Leptos 0.8 CSR 界面、单一状态与派生评测、浏览器存储与剪贴板 |
| `crates/langbridge-cli` | 命令解析、画像文件读写、本地 OpenAI 兼容代理 |

前端使用 Leptos 0.8 编译为 WebAssembly，样式为单文件设计系统，字体只使用 MiSans 子集，数字与代码使用等宽字体。

画像按模型分别持久化。Web 存在浏览器 localStorage，CLI 存在用户目录下的 `langbridge` 文件夹；画像可导出、导入与校验。

## 测试

```bash
cargo test --workspace
```

覆盖信号提取、校准拟合、四层路由、注入冲突、输出守卫、内置证据、地址归一与用量解析。

## 已知限制

- 内置数据来自模型公开 tokenizer 的真实计数，不是服务端账单。精确计费口径请使用 API 测量。
- 内置证据只覆盖三个模型。其他模型需要 API 测量，否则只按质量规则给语言建议，不输出成本结论。
- 非推理模型没有独立推理 Token，不适用于本工具的成本结论。

## 关于

- 源码仓库：<https://atomgit.com/openairymax/langbridge>
- 在线体验：<https://langbridge.traesite.cn>

OpenAirymax 开源极境

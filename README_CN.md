# cargo-bumpp

[![Release](https://github.com/imba97/cargo-bumpp/actions/workflows/release.yaml/badge.svg)](https://github.com/imba97/cargo-bumpp/actions/workflows/release.yaml)
[![crates.io](https://img.shields.io/crates/v/cargo-bumpp.svg)](https://crates.io/crates/cargo-bumpp)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](#许可)

> English: [README.md](README.md)

交互式版本号维护工具：选一个升级级别，改写清单与 lockfile，提交、打 tag、推送。
**零依赖** —— `cargo install` 是秒级，也没有需要审计的依赖树。

**它不负责发布。** 范围到 tag 为止，之后由 CI 接手。crates.io 的版本号不能删、不能覆盖，
所以「本地敲一条命令就发出去」这件事不该发生。

```console
$ cargo bumpp patch
  bumpp  /path/to/project
  crates  war3-archive, war3-core, war3-map  (shared version)

  war3-archive             0.0.2 -> 0.0.3
  war3-core                0.0.2 -> 0.0.3
  war3-map                 0.0.2 -> 0.0.3

  Cargo.toml
    [workspace.package]       line 6: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 11: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 13: 0.0.2 -> 0.0.3

  commit  chore: release v0.0.3
     tag  v0.0.3
    push  origin main and v0.0.3

? Bump? (Y/n) y
  wrote Cargo.toml
  commit chore: release v0.0.3
     tag v0.0.3
    push origin main, v0.0.3
```

## 目录

- [为什么再造一个](#为什么再造一个)
- [安装](#安装)
- [快速开始](#快速开始)
- [默认流程](#默认流程)
- [重新发布 tag](#重新发布-tag)
- [选项](#选项)
- [签名（GPG）](#签名gpg)
- [退出码](#退出码)
- [版本号藏在哪](#版本号藏在哪)
- [计划输出](#计划输出)
- [交互选择器](#交互选择器)
- [提交信息与 tag 名](#提交信息与-tag-名)
- [配置](#配置)
- [与 bumpp 的差异](#与-bumpp-的差异)
- [用做库](#用做库)
- [故障排查](#故障排查)
- [开发](#开发)
- [致谢](#致谢)
- [许可](#许可)

## 为什么再造一个

这条路已经很挤，所以这里给一份诚实的对比。

| | cargo-bumpp | [cargo-release](https://github.com/crate-ci/cargo-release) | [cargo-bump](https://crates.io/crates/cargo-bump) |
| --- | --- | --- | --- |
| 安装成本 | 秒级，无依赖 | 默认要编译 libgit2/OpenSSL | 秒级 |
| 交互式版本选择器 | 有 | 无 | 无 |
| 按提交历史判级别（`conventional`） | 有 | 无 | 无 |
| 共享 workspace 版本 + path 依赖版本 | 都管 | 都管 | 部分 |
| 失败回滚 | 有（push 之前） | 部分 | 无 |
| 发布到 registry | 有意不做 | 有 | 无 |
| changelog | 有意不做 | 可选 | 无 |
| PR 发布流程（`--pr`） | 不做 | 有 | 无 |

需要 changelog、需要每个 crate 独立发版、或者要工具替你发布 —— 那用 `cargo-release`。
想把常见路径（一个共享版本、提交、打 tag、推送）放进一个秒级安装的交互式工具里，
那用 `cargo-bumpp`。

## 安装

```bash
cargo install cargo-bumpp
```

装完 `cargo bumpp`（作为 Cargo 子命令）与 `bumpp`（直接调用）都能用，两者等价：

```bash
cargo bumpp patch     # 作为 Cargo 子命令
bumpp patch           # 直接调用
```

### 环境要求

- **Rust 1.74+** 用于构建与安装（即 MSRV）。
- 运行期需要 PATH 上有 **`cargo` 与 `git`**：两者都没有被链接进来 —— 版本图来自
  `cargo metadata`，所有 git 动作都是子进程。
- Linux / macOS / Windows。方向键选择器需要终端支持 ANSI 转义；其它情况自动降级为编号列表。

## 快速开始

```bash
cargo bumpp                        # 选择 → 提交 → tag → push
cargo bumpp patch                  # 跳过选择器，直接按 patch 升
cargo bumpp minor -y               # 连确认也跳过
cargo bumpp 1.0.0 --no-push        # 指定版本，只到 tag，先看一眼
cargo bumpp conventional           # 按提交历史判断 major/minor/patch
cargo bumpp --retag                # 重新发布上一个 tag：重建 tag 并强推
cargo bumpp --preid rc --release prepatch
cargo bumpp --no-tag --no-push     # 只改版本号
```

## 默认流程

```
  1. 展示版本选择器，用户选择          ← 唯一的交互
  2. 打印摘要（含每一处改动的行号），"Bump?" 确认一次
  3. 改写版本号（清单 + Cargo.lock）
  4. git commit   "chore: release v1.2.1"
  5. git tag      v1.2.1（附注 tag，附注信息复用提交信息）
  6. git push     分支，然后推这一个 tag
```

**第 3 到 5 步任意一步失败就回滚**：先删 tag、再 `git reset --hard` 回到运行前的提交、
最后按写文件之前的快照还原文件。

**第 6 步 push 失败不回滚。** 推送是第一个本地无法撤销的动作，而且推送被拒通常是远端在说不
（受保护分支、非快进、无权限）—— 重试或修好再推，都比删掉本地成果合理。本地提交与 tag 保留，
git 的报错原样打印，并给出可以直接照抄的重试命令。

## 重新发布 tag

tag 推上去之后流水线挂了，这个 tag 没法"再推一次"：**git 会跳过没有变化的 ref**，推送等于什么都没
做，流水线也不会再跑。`--retag` 就是手动该做的那两步 —— 重建 tag、强推 —— 同样有摘要和确认：

```console
$ cargo bumpp --retag
  bumpp  /path/to/project

    retag  v0.3.2  (annotated, keeps its message)
       to  1a2b3c4 chore: release v0.3.2 (HEAD, unchanged)
     push  --force origin refs/tags/v0.3.2

? Re-release? (Y/n) y
  retag   v0.3.2
  push    origin refs/tags/v0.3.2 (forced)
```

- **发布哪个**：`--retag <tag>` 用给定的 tag；不带参数就用**从 HEAD 可达的最近一个 tag** —— 和
  `conventional` 读取提交范围的"上一个 tag"是同一个定义 —— **但前提是这个 tag 已经在 HEAD 上**。
  如果 HEAD 已经走在这次发布前面，默认重发就会把已发布的版本号指到未发布的提交上，所以这种移动必须
  点名说清楚。一个 tag 都没有、或者给的 tag 不在本仓库里，都会报错并给出修法（`git fetch --tags`）。
- **是重建，不只是推送**：附注 tag 会保留原有附注信息，并生成一个新的 tag 对象（tagger 时间属于对象
  内容）；轻量 tag 则和 `git tag -f` 一样移到 HEAD。正是这个新对象让强推真的更新远端 ref，流水线才
  会重新跑。
- **其它什么都不做**：不改版本号、不写文件、不提交，也不需要工作区干净 —— 没有东西要回滚。当这次
  重新发布会把 tag 移到**另一个提交**上时，会明确警告。
- **`--no-push`** 只重建本地 tag，**`-y`** 跳过确认，与其它命令一致。强推被拒（tag 受保护、无权限）
  同样不回滚：打印 git 的报错和重试命令。
- **升级相关的选项不适用**：`--retag` 与 `--commit`、`--tag`、`--all`、`--recursive`、`--execute`、
  `--preid`、`--current-version`、`--commit-window`、`--lockfile`、`--print-commits`、`--verify`、
  `--ignore-scripts` 同时给出是用法错误，而不是悄悄忽略。`bumpp.toml` 里写了这些没关系 —— 仓库默认
  值不算"显式要求"。
- 如果重建出来的 tag 对象**完全没变**（附注 tag 在同一秒内重建），git 会说无事可推，工具会明确提示，
  而不是假装推过了。已经在 HEAD 上的轻量 tag 根本没法这样重发，提示里会给出"先删远端 tag 再推"的命令。

## 选项

| 选项 | 默认 | 作用 |
| --- | --- | --- |
| `--release <level>` | `prompt` | 升级级别或版本号，替代位置参数 |
| `--retag [tag]` | 上一个 tag（须在 HEAD 上） | 重新发布已有 tag：重建后强推，见[重新发布 tag](#重新发布-tag) |
| `--preid <preid>` | `beta` | 预发布标识 |
| `-a, --all` | `false` | 连同其它改动一起 `git add --all` 并提交 |
| `--git-check` / `--no-git-check` | 开 | 要求工作区干净 |
| `-c, --commit [msg]` / `--no-commit` | 开 | 提交，可选自定义提交信息 |
| `-t, --tag [name]` / `--no-tag` | 开 | 附注 tag，可选自定义 tag 名（模板） |
| `--sign` / `--no-sign` | 都不给 | 见[签名](#签名gpg) |
| `-p, --push` / `--no-push` | 开 | 推送分支与该 tag |
| `-y, --yes` | `false` | 跳过 `Bump?` 确认 |
| `-r, --recursive` | `false` | 也让各成员按自己的版本号升级 |
| `--verify` / `--no-verify` | 开 | `--no-verify` 时给 `git commit` 传 `--no-verify` |
| `--ignore-scripts` | `false` | 仅为与 bumpp 对齐而接受；Cargo 没有生命周期脚本 |
| `-x, --execute <command>` | — | 版本改完后、提交之前执行的命令 |
| `--current-version <v>` | 自动探测 | 显式指定当前版本 |
| `--print-commits` / `--no-print-commits` | 开 | 打印 `conventional` 依据的提交 |
| `--lockfile` / `--no-lockfile` | 开 | 是否跑 `cargo update --workspace` |
| `--commit-window <n>` | `100` | `conventional` 最多看多少条提交 |
| `--configFilePath <path>` | `bumpp.toml` | 自定义配置文件路径 |
| `-q, --quiet` | `false` | 只打印警告与错误 |
| `-h, --help`, `-V, --version` | | |

**短选项照抄 bumpp**，包括两个容易猜错的：`-r` 是 `--recursive`（不是 `--release`），
`-p` 是 `--push`（不是 `--preid`）。`cargo bumpp --help` 打印同一张表。

`--release` 取值：`major`、`minor`、`patch`、`next`、`conventional`、
`conventional-prerelease`、`premajor`、`preminor`、`prepatch`、`prerelease`、`as-is`、
`prompt`，或直接给版本号（`1.2.3`，允许写 `v1.2.3`）。

### 签名（GPG）

| 情况 | 行为 |
| --- | --- |
| `--sign` | 提交加 `--gpg-sign`，tag 加 `--sign` |
| 两个都不给 | **git 自己的配置说了算**：全局或仓库里开了 `commit.gpgsign` / `tag.gpgsign`，提交与 tag 就会走签名 —— 于是每次都要 GPG 授权 |
| `--no-sign` | 显式不签名：给 git 传 `-c commit.gpgsign=false` / `-c tag.gpgsign=false`，压过那两项配置 |

`--sign` 只是「要求签名」，而 `--no-sign` 才是「压过 git 原有默认」的那一个。
签名失败（没有可用密钥、PIN 输错、`gpg` 不可用）算作打 tag 失败，所以
**已经建好的提交会被一并撤掉**，不会留下签了一半的半成品。

## 退出码

| 码 | 含义 |
| --- | --- |
| `0` | 成功 |
| `1` | 某项检查失败；已回滚到干净状态 |
| `2` | 用法错误（未知选项、冲突的组合、配置文件写错） |
| `130` | 在提示处取消（选择器里 Ctrl+C，或在 `Bump?` 答 `n`） |
| 其他 | push 失败时透传 git 的退出码，此时本地提交与 tag **保留** |

## 版本号藏在哪

一次升级要改的位置比想象的多。以 `[workspace.package] version` 为唯一真源的 workspace：

```toml
[workspace.package]
version = "0.0.2"                                              # 1. 唯一真源

[workspace.dependencies]
war3-core = { path = "crates/war3-core", version = "0.0.2" }   # 2. 每个成员重复一遍（发布时用）
```

成员清单里指回工作区的 path 依赖是同一件事的第三种写法：

```toml
[dependencies]
war3-core = { path = "../war3-core", version = "0.0.2" }        # 3. 同值，另一种形状
```

工具用 `cargo metadata --no-deps --format-version 1` 找到这些位置 —— 它是**子进程，
不是 crate 依赖**，解析正确性由 Cargo 自己保证。改写时**逐行处理、保留原有格式**：
只替换版本字符串本身，缩进、注释、行尾、`rust-version`、`version.workspace = true`
全部原样保留。

| 工程形态 | 行为 |
| --- | --- |
| 有 `[workspace.package] version` | 改它，以及所有指向成员、且当前值等于它的 path 依赖 |
| 只有一个包，版本写在 `[package]` | 改它 |
| 多个包、各自独立版本、没有共享版本 | **报错退出**，列出找到的成员与版本；`--recursive` 可改为逐个升级 |

第三种情况不猜、也不挑一个成员改：改一个会让依赖它的成员失配，而这类错误要到发布时才显现。

### 只会报告、不会静默放过的情况

- `--recursive` 之外的成员自己写了不同的字面版本 → 警告，并说明可以用 `--recursive`
- path 依赖带了 path 却没有 `version` → 警告（`cargo publish` 无法改写它）
- 版本号跨行的写法（多行 inline table）→ 警告，说明**哪一处**定位不到
- 依赖方的版本要求不再接受新版本（`^0.0.2` 之于 `0.0.3`）→ 警告并列出

最后一条**这一版只检测与报告，不自动改写**：那会改变依赖解析行为，而一个悄悄放宽版本要求的
工具比一个直接列出来让人确认的工具更难信任。

## 计划输出

写文件之前先打印将要发生的事 —— **每一处都带行号**，漏没漏在代价还小的时候就看得见：

```
  Cargo.toml
    [workspace.package]       line 6: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 11: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 13: 0.0.2 -> 0.0.3
```

三成员 workspace 上跑一次 `cargo bumpp patch` 的完整输出，就是本文
[开头那段](#cargo-bumpp)。

这份输出就是「不需要干跑模式」的依据：唯一会静默出错的环节（漏改某一处）已经逐行列出来了；
剩下的风险是「写坏了」，那由回滚处理。同一份列表也是「定位不到的位置」的出口 ——
「它说这个文件它处理不了」在确认之前就有答案，而不是在写完之后。

## 交互选择器

十一项，顺序与 bumpp 一致。注意第二行 `conventional` 其实是 `conventional-prerelease`，
两者只靠右边显示的版本区分：

```
? Current version 0.0.2                ← 版本号显示为绿色
  up/down (or j/k), Enter to pick, Ctrl+C to cancel
>          next 0.0.3                 ← 选中项：加粗青色
           major 1.0.0                ← 未选中：暗色
           minor 0.1.0
           patch 0.0.3
    conventional 0.0.3
    conventional 0.0.3-beta.1
       pre-patch 0.0.3-beta.1
       pre-minor 0.1.0-beta.1
       pre-major 1.0.0-beta.1
           as-is 0.0.2
          custom ...
```

- **按键**：`↑`/`↓` 或 `j`/`k` 移动，`g`/`G` 跳首尾，数字键直接跳行，`Enter` 选中，`Ctrl+C` 取消。
- **`custom …` 行**复用菜单那套单键输入：这个模式下终端既不回显也不整行，所以由工具自己回显，
  `Backspace` 删改，`Enter` 结束，`Ctrl+C` 取消本次运行 —— 此时还没有写过任何文件。
  版本号是 ASCII，其它字符会被忽略，不会回显。
- **高亮**：光标所在行加粗青色，其余行压暗。颜色只在真的能输出转义序列时才用 ——
  输出被重定向、`cmd.exe` 不支持 ANSI、或 `--quiet` 时都会去掉，管道里拿到的始终是纯文本。
- **按终端高度滚动**：菜单最多占「终端行数 − 3」（表头、提示行、一行余量），最少 3 行。
  装不下时**最后一行显示 `↓ ...`**，光标停在其上方一行：指针下移到倒数第二行时窗口才开始上翻，
  收益是下一项在你选中它之前就已经可见。再往下翻，顶部出现 `↑ ...`；滚到列表末尾时
  `↓ ...` 消失，最后一行变回真实选项。终端高度拿不到时直接全部显示，不猜。

  8 行终端下、光标在第 4 项时它长这样：

  ```
  ? Current version 1.2.0 »
    up/down (or j/k), Enter to pick, Ctrl+C to cancel
    ↑ ...
            minor 1.3.0
            patch 1.2.1
  >          next 1.2.1
     conventional 1.2.1
    ↓ ...
  ```

- **没有终端时**：降级为编号列表 —— 输入编号、名字（`pre-m` 这类前缀也行），或直接回车取默认项。
  **无 TTY 时不会挂住**：读不到输入就按下面的消息报错退出。这条路径不带颜色、也不滚动，
  因为它的输出通常是给人或 CI 读的纯文本。

```
Cannot prompt for the version number because input or output has been disabled.
```

### 各项的确切含义

- `next` 在**稳定版**上等于 `patch`（0.0.2 → 0.0.3）；在**预发布**上递增预发布序号
  （0.0.3-rc.1 → 0.0.3-rc.2）。它**不会把预发布转正** —— 转正要靠 `custom` 手工填。
- `as-is` 原样不动版本号，只走 git 那几步。此时没有文件改动，提交会是空的，所以提交带
  `--allow-empty`。`as-is` 的意思是「版本不动，但我需要一个发布点」，那个提交就是 tag 指向的地方。
- 首次进入预发布是 `-beta.1` 而不是 `-beta.0`；当前版本已经是预发布时**沿用它的标识**
  （`1.2.1-rc.3` 继续用 `rc`，不会因为 `--preid beta` 而改成 `beta`）。
- `conventional` 扫**上一个 tag 到 HEAD** 的提交：有 breaking change 记 `major`，否则有 `feat`
  记 `minor`，都没有记 `patch`。窗口由 `--commit-window` 限制（默认 100），超出时会打印
  「还有多少条更早的提交没看」，而不是悄悄截断。
- 提交信息不符合 Conventional Commits 时**退化为 `patch`，不报错** —— 没有 `feat` 就是 patch。

## 提交信息与 tag 名

默认值，可在配置里覆盖：

| | 默认 |
| --- | --- |
| 提交信息 | `chore: release v{version}` |
| tag | `v{version}` |

模板 token 与 bumpp 一致：

| token | 含义 | 示例 |
| --- | --- | --- |
| `{version}` | 新版本号 | `1.2.3` |
| `{oldVersion}` | 旧版本号 | `1.2.2` |
| `{tag}` | 格式化后的 tag 名 | `v1.2.3` |
| `{releaseType}` | 升级类型（显式版本号时为空） | `patch` |
| `{major}` / `{minor}` / `{patch}` | 新版本的三段 | `1` / `2` / `3` |
| `{date}` | 当前日期（本地时区） | `2026-07-28` |

渲染规则三档：含任一命名 token 时只替换命名 token（`%s` 不生效）；否则有 `%s` 就替换 `%s`；
两者都没有则**把新版本号追加到末尾** —— 所以 `--commit "chore: release v"` 是合法写法。
tag 名**先解出来**，其结果再作为 `{tag}` 供提交信息等模板使用。

```bash
cargo bumpp --commit "chore: release {tag}" --tag "{version}"
```

## 配置

**配置文件是可选的**：没有 `bumpp.toml` 时全部取内置默认值，工具完整可用。找的是 workspace
根目录下的 `bumpp.toml`，优先级 **命令行 > 环境变量 > bumpp.toml > 内置默认值**。

```toml
# bumpp.toml（全部可选项；照抄这份等于什么都没改）
commit = true
tag = true
push = true
commit-message = "chore: release v{version}"
tag-name = "v{version}"
preid = "beta"
sign = false

# 另外这几个也是可配置的
all = false
git-check = true
verify = true
lockfile = true
recursive = false
quiet = false
print-commits = true
commit-window = 100
```

常见的两种改法：

```toml
push = false                           # 只想本地确认后再推
commit-message = "release: {version}"  # 换提交信息风格
```

**未知键会报错而不是忽略**：一个拼错的 `commti-message` 如果被静默忽略，表现就是「配置了但没生效」，
而人会先去怀疑工具。报错里会列出全部已知键及其默认值。`--execute` 这类会执行命令的选项
**不能写进配置文件**，必须在命令行给。

只解析这个扁平 schema 用到的 TOML 子集（`key = "string"`、`key = true/false`、`key = 123`、
`#` 注释、空行），几十行，不是通用 TOML 实现 —— 与改写 `Cargo.toml` 时的逐行方案同一取向。

### 环境变量

`BUMPP_COMMIT`、`BUMPP_TAG`、`BUMPP_PUSH`、`BUMPP_PREID`、`BUMPP_COMMIT_MESSAGE`，
优先级高于配置文件，方便 CI 覆盖仓库里的默认值（典型用法是覆盖成不推送）。

## 与 bumpp 的差异

设计文档里已经写明的差异（`--pr` 不做、配置文件用静态 TOML 而不是可执行模块、工作区检查
默认打开、推具体 tag 而不是 `--tags`）都按文档实现。除此之外还有几处：

| 地方 | bumpp | 本工具 | 为什么 |
| --- | --- | --- | --- |
| `--no-commit` | 被 tag/push 通过 `\|\|` 拉回真，几乎无效 | 显式 `--no-commit` 就是生效，同时**不再 tag**；`--no-commit --tag` 判为用法错误 | 用户的显式要求不该被静默覆盖；没有提交时 tag 指向哪儿？ |
| 在 `Bump?` 处答 `n` | 退出码 1 | 退出码 130 | 按退出码表：在提示处取消 |
| `conventional` 的窗口 | 上一个 tag 到 HEAD，无上限 | 同上，但受 `--commit-window` 限制并在截断时提示 | 窗口要能配置 |
| tag 已存在 | 提交、打 tag 时才失败并回滚 | **写文件之前**就检查并报错 | 能早失败就不要晚失败 |
| tag 推上去之后流水线挂了 | 只能手动推、或者删掉远端 tag 再推 | `--retag` 先展示 tag，再重建并强推 | 没动过的 ref 推了等于没推，必须重建 tag 流水线才会再跑 |
| 推 tag | `git push --tags`（其它本地 tag 也一起推走） | `git push <remote> refs/tags/<tag>` | 在跑发布 workflow 的仓库里，推走别人的旧 tag 会触发一次意外发布 |
| 新增选项 | — | `--lockfile`、`--commit-window`、`--no-sign`、`--no-print-commits`、`--no-all` | 每个布尔开关都有否定形式，方便临时覆盖配置文件 |

**有意不做**的事：`--pr` 的完整 PR 发布流程；自动改写 workspace 之外的 `^x.y.z` 要求；
changelog 生成；发布到 registry。

## 用做库

```rust
use cargo_bumpp::{Prompt, Selection, Level};

// 自己的 prompt：确定性地回答，或在别的界面里实现同样的交互
let mut prompt = cargo_bumpp::prompt::ScriptedPrompt::new(
    vec![Selection::Level(Level::Patch)],
    vec![true], // 确认
);
let args = vec!["--no-push".to_string()];
cargo_bumpp::run_in(std::path::Path::new("."), &args, Some(&mut prompt))?;
```

`Prompt` 是一个 trait（`select_release` / `confirm`），默认实现是终端；不想交互时传
`RefusePrompt`，它会带着「input or output has been disabled」报错而不是挂住。
只想用其中一部分也可以 —— `semver`、`tokens`、`toml_line`、`plan`、`workspace` 都是公开的。

## 故障排查

**它拒绝运行：`Git working tree is not clean`**
这道检查默认开着：一个会写文件、建提交、打 tag 的工具，在脏工作区里分不清哪些是自己改的，
回滚时分不清哪些该撤。先提交或 stash；如果你接受这个代价，用 `--no-git-check` 跳过。

**每次运行都要我的 GPG 密钥**
你的 git 配了签名（`commit.gpgsign` / `tag.gpgsign`）。单次加 `--no-sign`，或在
`bumpp.toml` 里设 `sign` —— 见[签名](#签名gpg)。

**提交失败：`user.email` 未配置**
`git commit` 需要身份，工具不会替你编一个。设置 git 的 `user.name` / `user.email`
后重试 —— 工作区已经回滚，没有半成品。

**`Cannot prompt for the version number because input or output has been disabled.`**
没有终端可以问，工具选择报错而不是挂住。给一个级别（`cargo bumpp patch`）或用
`--release <level>`；再加 `-y` 可同时跳过确认。

**push 失败了，但提交和 tag 还在本地**
这是有意的 —— 见[默认流程](#默认流程)。报错里会打印可直接重试的 `git push` 命令，
以及本地如何撤销。

**`cargo build` 明明改了源码却说 `Fresh`**
不是本工具的问题，但开发时确实绊过我们一次：cargo 的指纹缓存失效时，
`cargo clean -p cargo-bumpp` 强制重编即可。

## 开发

```bash
cargo test     # 单元测试 + 端到端测试
cargo fmt      # 无 rustfmt.toml，即默认风格
cargo clippy --all-targets -- -D warnings
```

在本仓库里，`cargo bumpp` 跑的是当前工作区而不是装好的快照 ——
[`.cargo/config.toml`](.cargo/config.toml) 里的一行 `[alias]` —— 所以不用先安装就能用它发布自己。
出了这个目录，`cargo bumpp` 就需要 `cargo install cargo-bumpp`（或在检出目录里
`cargo install --path .`）。

顶部徽章属于那条只在发布 tag 上运行的流水线；检查本身在
[`.github/workflows/ci.yaml`](.github/workflows/ci.yaml) 里，内容就是上面这三条命令。

端到端测试会真的建临时 Cargo workspace 和 git 仓库，跑真的 `cargo metadata`、真的
`git commit`，覆盖回滚、push 失败、脏工作区、无 TTY、pre-commit hook 拒绝、签名失败等路径。
测试用的 git 配置全部隔离在临时文件里（`GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM`），
不会读也不会改你机器上的配置 —— 也因此不会弹出任何 GPG 窗口。

代码结构：

| 文件 | 内容 |
| --- | --- |
| `src/cli.rs` | 命令行解析与 `--help` |
| `src/options.rs` | 默认值合并，以及「tag/push 决定 commit 默认值」这类跨选项规则 |
| `src/workspace.rs` | `cargo metadata` 的读取（自写的 JSON 解析器在 `src/json.rs`） |
| `src/toml_line.rs` | 逐行的 `Cargo.toml` 读取与改写，保留原格式，定位不到的写法会报告 |
| `src/plan.rs` | 计划：改哪些文件、哪些行、改成什么；警告也在这里产生 |
| `src/app.rs` | 流程编排、回滚、conventional 判断、git 步骤 |
| `src/prompt.rs`、`src/sys.rs` | 选择器、确认，以及终端/平台的底层处理 |
| `src/report.rs` | 计划输出与摘要 |
| `src/git.rs` | 所有 git 子进程调用 |
| `src/config.rs` | `bumpp.toml` 与 `BUMPP_*` |
| `src/semver.rs` | semver 解析、比较、`inc`（对齐 node-semver），以及版本要求检查 |

欢迎提 issue 与 PR：大改动先开 issue 讨论，并请保持依赖数为零。

## 致谢

- [bumpp](https://github.com/antfu-collective/bumpp)
- [prompts](https://github.com/terkelg/prompts)

## 许可

MIT —— 见 [LICENSE](LICENSE)。

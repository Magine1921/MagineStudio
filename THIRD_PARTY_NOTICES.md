# Third-party notices

MagineStudio is an independent community project. It is not affiliated with,
endorsed by, or sponsored by MiniMax, Comfy Org, NVIDIA, Microsoft, or the
other projects and companies referenced below.

## MiniMax H3 model weights

MiniMax H3 model weights are **not included** in this repository or in the
MagineStudio source-code license. When a user chooses to install a model,
MagineStudio downloads the selected files from the publisher's repository.

Use, modification, and redistribution of those files are governed by the
[MiniMax H3 model repository and its current license](https://huggingface.co/MiniMaxAI/MiniMax-H3).
Users must review and comply with that license, including any use, territory,
distribution, and attribution restrictions, before downloading or using the
weights.

ComfyUI-ready model files are referenced from the
[Comfy Org MiniMax H3 repository](https://huggingface.co/Comfy-Org/MiniMax-H3).
That repository may contain files with more than one upstream license. The
license accompanying each downloaded file controls.

## ComfyUI

MagineStudio controls ComfyUI as a separate local process through its HTTP
and WebSocket APIs. ComfyUI is not copied into this source repository.
MagineStudio can download an official ComfyUI Windows portable archive at the
user's request.

ComfyUI is distributed under the
[GNU General Public License v3.0](https://github.com/Comfy-Org/ComfyUI/blob/master/LICENSE).
Anyone redistributing ComfyUI, including as part of a combined installer or
offline bundle, is responsible for satisfying the GPL-3.0 source and notice
requirements for the ComfyUI portion.

## Application dependencies

MagineStudio is built with Tauri, React, Vite, Vitest, Lucide React, Tokio,
Reqwest, Serde, and other open-source libraries. Exact dependency versions
are recorded in `package-lock.json` and `src-tauri/Cargo.lock`. Each dependency
remains governed by its own upstream license and copyright notices.

## Trademarks

MiniMax, ComfyUI, NVIDIA, CUDA, Windows, React, Tauri, and other names are the
property of their respective owners. Their use in this project is solely to
identify compatibility or dependencies.

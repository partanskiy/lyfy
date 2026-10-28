# Source and distribution licenses

The lyfy authors' own source files are licensed under MIT; see LICENSE.
The owner explicitly approved the GPL dependencies and distribution terms for
linked builds on 2026-10-28.

The executable and library build link to the GPL-3.0 Rust alpm bindings and
dynamically to the system libalpm. Distribute the combined build under
GPL-3.0-only, retaining the MIT notices for lyfy's own code. Dynamic linking
does not make the combined build MIT-only.

Release source archives include locked Rust dependency sources and their
licenses under vendor/. THIRD_PARTY_LICENSES.txt includes the complete GPL
text and exact dependency notices. Binary archives and AUR packages include
this file and this distribution notice. Keep the corresponding source archive
available alongside each binary release.

Native libraries remain system dependencies managed by pacman. Source archives
also identify the installed native versions and include the corresponding
pacman/libalpm source and Arch packaging materials under native-sources/.
The shipped executable is built for Arch Linux x86_64 and the detected libalpm
ABI; a future libalpm ABI change requires a compatible rebuild.

## Commit Official #1
- Deprecated old WM and Main
- Added new WM and Main
- update syscalls
- split some apps into the programs/ folder
- add a test app to test ui via syscalls
- WASM compiler will be removed and deprecated soon, as it is beginning to be replaced with ELF
## Commit Official #2
- move deprecated memory file from src to kernel/src/memory.deprecated
- this deprecated memory file will be removed in future updates when current memory handler will be deemed stable
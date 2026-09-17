"""Native offline payload acceptance. Run with the bundled Python (-B).

Uses the application's stdio arguments, isolated workspaces and real MCP.
Writes synthetic Shell output for the WKWebView combined acceptance.
"""
import json
import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile
from importlib.metadata import version
from typing import Any

import anyio
from mcp import ClientSession, types
from mcp.client.stdio import StdioServerParameters, stdio_client

URI = "window://io.github.a2c-smcp.tfbash/shell-overview"
TOOLS = {"shell_open", "shell_exec", "shell_read", "shell_write", "shell_signal", "shell_list", "shell_close"}


async def check_workspace(root: Path, interpreter: Path) -> str:
    updates: list[str] = []
    updated = anyio.Event()

    async def notification(message: Any) -> None:
        if isinstance(message, types.ServerNotification) and isinstance(message.root, types.ResourceUpdatedNotification):
            updates.append(str(message.root.params.uri))
            updated.set()

    parameters = StdioServerParameters(
        command=str(interpreter),
        args=["-B", "-m", "tfbash_mcp", "--transport", "stdio", "--runtime-profile", "auto", "--host-profile", "ide", "--workspace-root", str(root)],
        cwd=str(root), env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
    )
    async with stdio_client(parameters) as (reader, writer), ClientSession(reader, writer, message_handler=notification) as session:
        async def call(name: str, **arguments: Any) -> dict[str, Any]:
            result = await session.call_tool(name, arguments)
            assert not result.isError, (name, result)
            assert result.structuredContent is not None, name
            return result.structuredContent

        async def read_all(shell_id: str, exec_id: str) -> str:
            cursor, output = 0, ""
            # Each read blocks on output/completion; immediately drain available pages.
            # No timer or sleep probes the execution state.
            with anyio.fail_after(30):
                while True:
                    result = await call("shell_read", shell_id=shell_id, exec_id=exec_id, cursor=cursor, wait_ms=30_000)
                    output += result["output"]
                    previous, cursor = cursor, result["next_cursor"]
                    if result["status"] != "running" and cursor == previous:
                        return output

        initialized = await session.initialize()
        assert initialized.capabilities.resources.subscribe
        assert {tool.name for tool in (await session.list_tools()).tools} == TOOLS
        resources = (await session.list_resources()).resources
        assert len(resources) == 1 and str(resources[0].uri) == URI
        assert resources[0].mimeType == "text/markdown"
        uri = resources[0].uri
        assert "No active Shells." in (await session.read_resource(uri)).contents[0].text
        await session.subscribe_resource(uri)
        context = await call("shell_list")
        assert Path(context["runtime"]["default_cwd"]).resolve() == root.resolve()
        opened = await call("shell_open")
        shell_id = opened["shell_id"]
        with anyio.fail_after(5):
            await updated.wait()
        assert updates[-1] == URI
        dialect = context["runtime"]["dialect"]
        # Use the bundled interpreter so the host needs no Python installation.
        program = "import os; print(os.getcwd()); print('A/B'*66667+' token=embedded-secret fixture')"
        if dialect == "pwsh":
            command = "& '" + str(interpreter).replace("'", "''") + "' -B -c '" + program.replace("'", "''") + "'"
            stdin_command = "$value = [Console]::ReadLine(); Write-Output \"stdin:$value\""
        else:
            command = f"{shlex.quote(str(interpreter))} -B -c {shlex.quote(program)}"
            stdin_command = "IFS= read -r value; printf 'stdin:%s' \"$value\""
        executed = await call("shell_exec", shell_id=shell_id, command=command, yield_ms=1000)
        output = await read_all(shell_id, executed["exec_id"])
        assert str(root.resolve()) in output and "A/B" * 66667 in output
        assert shell_id in (await session.read_resource(uri)).contents[0].text
        waiting = await call("shell_exec", shell_id=shell_id, command=stdin_command, yield_ms=0)
        written = await call("shell_write", shell_id=shell_id, exec_id=waiting["exec_id"], text="ready\n")
        assert written["accepted_bytes"] > 0
        assert "stdin:ready" in await read_all(shell_id, waiting["exec_id"])
        waiting = await call("shell_exec", shell_id=shell_id, command=stdin_command, yield_ms=0)
        signalled = await call("shell_signal", shell_id=shell_id, exec_id=waiting["exec_id"], signal="kill")
        assert signalled["status"] == "delivered"
        await read_all(shell_id, waiting["exec_id"])
        await session.unsubscribe_resource(uri)
        count = len(updates)
        closed = await call("shell_close", shell_id=shell_id)
        assert closed["cleanup_complete"] is True
        assert "No active Shells." in (await session.read_resource(uri)).contents[0].text
        # Bounded negative observation, not polling: await an unexpected notification.
        updated = anyio.Event()
        with anyio.move_on_after(0.5):
            await updated.wait()
        assert len(updates) == count
        return output


async def main() -> None:
    assert version("tfbash-mcp") == "0.2.1"
    assert sys.version_info[:3] == (3, 12, 14)
    with tempfile.TemporaryDirectory(prefix="tfbash-upgrade-") as directory:
        root = Path(directory)
        # tfbash's isolated bootstrap may create bytecode despite the outer -B.
        # Exercise a byte-identical copy so acceptance never dirties the bundle.
        source = Path(sys.executable).resolve()
        python_root = source.parent if sys.platform == "win32" else source.parent.parent
        shutil.copytree(python_root, root / "python", symlinks=True)
        interpreter = root / "python" / source.relative_to(python_root)
        a, b = root / "computer-a", root / "computer-b"
        a.mkdir(); b.mkdir()
        with anyio.fail_after(90):
            output = await check_workspace(a, interpreter)
            other = await check_workspace(b, interpreter)
        assert str(a) not in other and str(b) not in output
    report = {"passed": True, "tfbash": version("tfbash-mcp"), "platform": sys.platform,
              "python": sys.version, "tools": sorted(TOOLS), "shellOutput": output}
    Path(sys.argv[1]).write_text(json.dumps(report), encoding="utf-8")
    print(f"PASS: {sys.argv[1]}")


if __name__ == "__main__":
    anyio.run(main)

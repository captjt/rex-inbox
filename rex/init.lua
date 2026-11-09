-- rex-inbox's Rex action, for the machine the hub runs on. Load it from
-- your init.lua:
--
--   dofile(os.getenv("HOME") .. "/src/rex-inbox/rex/init.lua")
--
-- inbox_rpc takes one request to the hub, req={"method", "params"} as JSON,
-- and returns the hub's answer as JSON in `response`. Agents on your other
-- Rex hosts use it through `rex -S HOST do inbox_rpc`, which is what the
-- rex-inbox CLI does on a machine without a hub of its own. Rex decides who
-- may call it: the same people who may run commands on this server.

local HOME = os.getenv("HOME")

local function find_bin()
  local candidates = {
    HOME .. "/.cargo/bin/rex-inbox",
    "/opt/homebrew/bin/rex-inbox",
    "/usr/local/bin/rex-inbox",
  }
  -- Not in the list itself: a nil would end ipairs there.
  local override = os.getenv("REX_INBOX_BIN")
  if override and override ~= "" then
    table.insert(candidates, 1, override)
  end
  for _, path in ipairs(candidates) do
    local f = io.open(path, "r")
    if f then
      f:close()
      return path
    end
  end
end

rex.action({
  name = "inbox_rpc",
  title = "rex-inbox: a request to the hub",
  args = { req = "string" },
  run = function(_, a)
    local bin = find_bin()
    if not bin then
      error("rex-inbox isn't installed on this machine", 0)
    end
    local env = { HOME = HOME, PATH = "/usr/bin:/bin:/usr/sbin:/sbin" }
    -- A hub with its state elsewhere (a test hub, say).
    for _, k in ipairs({ "REX_INBOX_STATE", "REX_INBOX_SOCKET" }) do
      env[k] = os.getenv(k)
    end
    local res = rex.exec.start({ bin, "rpc", a.req }, { cwd = HOME, env = env }):wait()
    if not res.ok then
      local err = (res.stderr ~= "" and res.stderr) or res.stdout or ""
      error("rex-inbox rpc: " .. err, 0)
    end
    -- As a string: decoding to Lua and back would lose nulls and empty lists.
    return { response = res.stdout }
  end,
})

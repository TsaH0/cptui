-- Tiny HTTP/1.1 server on vim.uv. Enough for Competitive Companion and the
-- cptui browser extension: one request per connection, Content-Length bodies.
local M = {}

local MAX_BODY = 20 * 1024 * 1024

local REASONS = {
  [200] = 'OK', [202] = 'Accepted', [204] = 'No Content', [400] = 'Bad Request',
  [403] = 'Forbidden', [404] = 'Not Found', [413] = 'Payload Too Large',
  [500] = 'Internal Server Error',
}

local function respond(client, status, headers, body)
  body = body or ''
  local lines = { ('HTTP/1.1 %d %s'):format(status, REASONS[status] or 'OK') }
  headers = headers or {}
  headers['Content-Length'] = tostring(#body)
  headers['Connection'] = 'close'
  for k, v in pairs(headers) do lines[#lines + 1] = k .. ': ' .. v end
  client:write(table.concat(lines, '\r\n') .. '\r\n\r\n' .. body, function()
    if not client:is_closing() then client:close() end
  end)
end

--- Parse a complete request from `buf`, or return nil if more bytes are needed.
--- Returns req, err (err is an HTTP status for malformed/oversized requests).
function M.parse(buf)
  local head_end = buf:find('\r\n\r\n', 1, true)
  if not head_end then
    return nil, #buf > 64 * 1024 and 400 or nil
  end
  local head = buf:sub(1, head_end - 1)
  local method, target = head:match('^(%u+) (%S+) HTTP/1%.[01]')
  if not method then return nil, 400 end
  local headers = {}
  for line in head:gmatch('\r\n([^\r\n]+)') do
    local k, v = line:match('^([^:]+):%s*(.-)%s*$')
    if k then headers[k:lower()] = v end
  end
  if headers['transfer-encoding'] then return nil, 400 end
  local len = tonumber(headers['content-length'] or '0') or 0
  if len > MAX_BODY then return nil, 413 end
  local body_start = head_end + 4
  if #buf - body_start + 1 < len then return nil end
  return {
    method = method,
    path = target:gsub('%?.*$', ''),
    headers = headers,
    body = buf:sub(body_start, body_start + len - 1),
  }
end

--- Start listening. `handler(req)` returns status, headers, body.
--- Handlers run on the main loop (vim.schedule), so they may call any API.
function M.listen(host, port, handler)
  local server = vim.uv.new_tcp()
  local ok, err = server:bind(host, port)
  if not ok then
    server:close()
    return nil, err
  end
  ok, err = server:listen(64, function(lerr)
    if lerr then return end
    local client = vim.uv.new_tcp()
    server:accept(client)
    local buf = ''
    client:read_start(function(rerr, chunk)
      if rerr or not chunk then
        if not client:is_closing() then client:close() end
        return
      end
      buf = buf .. chunk
      local req, status = M.parse(buf)
      if status then
        client:read_stop()
        respond(client, status)
      elseif req then
        client:read_stop()
        vim.schedule(function()
          local ok2, s, h, b = pcall(handler, req)
          if not ok2 then
            s, h, b = 500, {}, tostring(s)
          end
          respond(client, s, h, b)
        end)
      end
    end)
  end)
  if not ok then
    server:close()
    return nil, err
  end
  return server
end

return M

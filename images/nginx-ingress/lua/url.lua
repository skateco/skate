local _M = {}
function _M.urldecode(s)
    s = s:gsub('+', ' ')
         :gsub('%%(%x%x)', function(h)
                             return string.char(tonumber(h, 16))
                           end)
    return s
end

function _M.urlencode(str)
    if (str) then
        str = string.gsub (str, "\n", "\r\n")
        str = string.gsub (str, "([^%w ])",
            function (c) return string.format ("%%%02X", string.byte(c)) end)
        str = string.gsub (str, " ", "+")
   end
   return str
end
  
function _M.parseurl(s)
    s = s:match('%s+(.+)')
    local ans = {}
    for k,v in s:gmatch('([^&=?]-)=([^&=?]+)' ) do
      ans[ k ] = urldecode(v)
    end
    return ans
end

return _M
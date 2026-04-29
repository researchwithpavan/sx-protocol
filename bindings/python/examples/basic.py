from sx import SxMessage, sx_version

print(sx_version())
msg = SxMessage.parse_text('{name:"Asha",active:true}')
print(msg.to_text())
print(msg.logical_hash().hex())
msg.close()

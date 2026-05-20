import struct
import os

def main():
    if not os.path.exists('icon.ico'):
        print("icon.ico not found")
        return
        
    with open('icon.ico', 'rb') as f:
        data = f.read()
    
    # Read ICO header
    reserved, type_, count = struct.unpack('<HHH', data[:6])
    if count == 0:
        print("No images in ICO")
        return
        
    # Read first entry
    width, height, colors, reserved2, planes, bpp, size, offset = struct.unpack('<BBBBHHII', data[6:22])
    
    # Extract raw image data
    img_data = data[offset : offset + size]
    
    # Save as PNG
    with open('icon.png', 'wb') as f:
        f.write(img_data)
    print("Extracted icon.png successfully!")

if __name__ == '__main__':
    main()

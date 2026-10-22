#!/usr/bin/env python3
from PIL import Image, ImageDraw, ImageFont
import os

# Color scheme
BG_COLOR = (26, 26, 46)  # #1A1A2E
D_COLOR = (53, 201, 151)  # #35C997 - Teal accent

def create_icon(size, round_corners=False):
    """Create a Dash icon with 'D' letter"""
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Background circle
    margin = 2
    circle_bbox = [margin, margin, size - margin, size - margin]
    draw = ImageDraw.Draw(img)
    draw.ellipse(circle_bbox, fill=(26, 26, 46, 255))
    
    # Draw letter D
    # Outer D shape
    center = size // 2
    radius = size // 2 - 6
    
    # Outer D - large curve
    # Draw outer circle
    draw.ellipse([center - radius, center - radius, center + radius, center + radius], 
                 fill=(26, 26, 46, 255), outline=None)
    
    # Draw D shape - outer
    # Draw a D shape by drawing a filled circle and then cutting out the left part
    # Actually simpler: draw the D shape directly
    draw = ImageDraw.Draw(img)
    
    # Background circle
    draw.ellipse([6, 6, size-6, size-6], fill=(26, 26, 46, 255))
    
    # Draw D shape - outer curve
    # D is a circle with a straight line on the left
    center = size // 2
    r = size // 2 - 8
    
    # Draw the D shape - filled circle minus left portion
    # Draw outer circle
    draw.ellipse([center-r, center-r, center+r, center+r], fill=D_COLOR)
    
    # Cut out left portion to make D shape
    # Draw a rectangle covering left half to erase it
    draw.rectangle([center-r, center-r, center, center+r], fill=(26, 26, 46, 255))
    
    # Draw inner counter of D (the hole)
    inner_r = r // 2
    draw.ellipse([center-inner_r, center-inner_r, center+inner_r, center+inner_r], fill=(26, 26, 46, 255))
    
    # Round corners for adaptive icon
    if False:  # not adaptive icon
        pass
    
    return img

# Better D drawing
def create_dash_icon(size):
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Background circle - dark background
    margin = 4
    draw.ellipse([4, 4, size-4, size-4], fill=(26, 26, 46, 255))
    
    # Draw the letter D in teal
    # We'll draw a D shape: circle with left portion cut out
    cx = size // 2
    cy = size // 2
    r = size // 2 - 10
    
    # Draw teal D shape
    # Outer circle in teal
    draw.ellipse([cx-50, cy-50, cx+50, cy+50], fill=(53, 201, 151, 255))
    
    # Cut out left side to make D shape
    draw.rectangle([cx-50, cy-50, cx, cy+50], fill=(26, 26, 46, 255))
    
    # Inner counter (hole)
    inner_r = 20
    draw.ellipse([cx-inner_r, cy-inner_r, cx+inner_r, cy+inner_r], fill=(26, 26, 46, 255))
    
    return img

# Better version - cleaner D
def create_clean_dash_icon(size):
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Background - full circle in dark
    margin = 2
    draw.ellipse([margin, margin, size-margin, size-margin], fill=(26, 26, 46, 255))
    
    # Draw D shape
    cx = size // 2
    cy = size // 2
    r = size // 2 - 6
    
    # Outer teal circle
    draw.ellipse([cx-r, cy-r, cx+r, cy+r], fill=(53, 201, 151, 255))
    
    # Cut left half to make D
    draw.rectangle([cx-r, cy-r, cx, cy+r], fill=(26, 26, 46, 255))
    
    # Inner counter
    inner_r = r // 2
    draw.ellipse([cx-inner_r, cy-inner_r, cx+inner_r, cy+inner_r], fill=(26, 26, 46, 255))
    
    return img

# Generate all densities
sizes = {
    'mdpi': 48,
    'hdpi': 72,
    'xhdpi': 96,
    'xxhdpi': 144,
    'xxxhdpi': 192,
}

# Also need round versions
# For round icons, we'll make them circular

base_path = "/home/ubuntu/dash-essentials-rebase/android/package/res"

# Clean existing
for dpi in ['mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi']:
    for variant in ['', '_round']:
        path = f"android/package/res/mipmap-{dpi}/ic_launcher{variant}.png"
        if os.path.exists(path):
            os.remove(path)

# Generate regular icons
for dpi, size in sizes.items():
    # Regular (square with rounded corners)
    img = create_clean_dash_icon(size)
    path = f"{base_path}/mipmap-{dpi}/ic_launcher.png"
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path)
    print(f"Created {path} ({size}x{size})")
    
    # Round version - fully circular
    round_img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # For round, just make it fully circular
    round_img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(round_img)
    margin = 0
    draw.ellipse([0, 0, size, size], fill=(26, 26, 46, 255))
    
    cx = size // 2
    cy = size // 2
    r = size // 2 - 6
    
    draw.ellipse([cx-r, cy-r, cx+r, cy+r], fill=(53, 201, 151, 255))
    draw.rectangle([cx-r, cy-r, cx, cy+r], fill=(26, 26, 46, 255))
    inner_r = r // 2
    draw.ellipse([cx-inner_r, cy-inner_r, cx+inner_r, cy+inner_r], fill=(26, 26, 46, 255))
    
    path = f"{base_path}/mipmap-{dpi}/ic_launcher_round.png"
    os.makedirs(os.path.dirname(path), exist_ok=True)
    round_img.save(path)
    print(f"Created {path} ({size}x{size})")

print("Done!")

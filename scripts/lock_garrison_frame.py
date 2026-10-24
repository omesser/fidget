#!/usr/bin/env python3
"""
Lock crop/scale/placement parameters for Timber Wolf garrison frame 0 to match pack idle-0.

This script:
1. Loads the capture frame 0 and pack idle-0 reference
2. Removes background/UI from capture
3. Iteratively tunes crop, scale, and position to match the reference
4. Saves lock parameters as JSON
5. Generates visual diagnostics

v7b: v2 cavity-removal logic + enlarged torso protection mask (65%×55% ellipse, centered)
     FIXED: Protection mask was not being created due to astype() copy bug
"""

import cv2
import numpy as np
import json
from pathlib import Path

# Paths
CAPTURE_FRAME = Path("/workspace/docs/pr/tw-garrison-v2/capture-frame0.png")
PACK_IDLE = Path("/workspace/characters/timber-wolf/frames/idle-0.png")
OUTPUT_DIR = Path("/workspace/docs/pr/tw-garrison-v2")
LOCK_JSON = OUTPUT_DIR / "frame0-lock.json"

# Pack canvas size (confirmed)
CANVAS_WIDTH = 176
CANVAS_HEIGHT = 160


def remove_background(img):
    """
    Remove background from capture frame using multi-stage color and threshold filtering.
    Removes Sketchfab blueprint artifacts and reduces fringing.
    
    v7b: v2 cavity removal + enlarged torso protection mask (65%×55% ellipse, centered)
         FIXED: Protection mask creation bug (astype copy issue)
    """
    if img.shape[2] == 4:
        rgb = img[:, :, :3]
    else:
        rgb = img
    
    h, w = rgb.shape[:2]
    
    # Convert to grayscale
    gray = cv2.cvtColor(rgb, cv2.COLOR_BGR2GRAY)
    
    # Step 1: Identify the mech by brightness
    # The mech is brighter than the very dark background (> 25)
    # but not as bright as UI chrome (< 200)
    mask_not_too_dark = gray > 25
    mask_not_too_bright = gray < 200
    mask = mask_not_too_dark & mask_not_too_bright
    
    # Step 2: Remove UI regions (top and bottom 12% of frame)
    ui_region = np.ones_like(mask, dtype=bool)
    ui_region[int(h*0.12):int(h*0.88), :] = False
    mask[ui_region] = False
    
    # Convert to uint8 for OpenCV
    mask_uint = mask.astype(np.uint8) * 255
    
    # Step 3: Morphological cleanup - CAREFULLY
    # Close small gaps, but don't expand too much
    kernel_small = np.ones((5, 5), np.uint8)
    mask_uint = cv2.morphologyEx(mask_uint, cv2.MORPH_CLOSE, kernel_small, iterations=2)
    
    # Step 4: Find and keep only the main contour (the mech)
    contours, _ = cv2.findContours(mask_uint, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    
    if contours and len(contours) > 0:
        # Find the largest reasonably-centered contour
        img_center_x = w / 2
        img_center_y = h / 2
        
        def score_contour(contour):
            area = cv2.contourArea(contour)
            if area < 1000:  # Too small
                return -1
            M = cv2.moments(contour)
            if M["m00"] == 0:
                return -1
            cx = M["m10"] / M["m00"]
            cy = M["m01"] / M["m00"]
            # Penalize contours far from center
            dist = np.sqrt((cx - img_center_x)**2 + (cy - img_center_y)**2)
            # Score favors large, centered contours
            return area * 1000 / (dist + 100)
        
        scored = [(score_contour(c), c) for c in contours]
        scored = [(s, c) for s, c in scored if s > 0]
        
        if scored:
            best_contour = max(scored, key=lambda x: x[0])[1]
            
            # Create mask from this contour ONLY
            mask_uint = np.zeros((h, w), dtype=np.uint8)
            cv2.drawContours(mask_uint, [best_contour], -1, 255, -1)
    
    # Step 4.5: CREATE TORSO PROTECTION MASK
    # This is a hard protect region to prevent v2 cavity removal from punching torso holes
    # v7b: ENLARGED to generously cover cockpit dome + torso grille + hip junction
    # FIX: Create mask as uint8 first so cv2.ellipse can modify it in-place
    protect_mask_uint = np.zeros((h, w), dtype=np.uint8)
    protect_mask_params = {}
    
    # Find the center and bounds of the current mask
    coords = cv2.findNonZero(mask_uint)
    if coords is not None:
        x, y, mw, mh = cv2.boundingRect(coords)
        center_x = x + mw // 2
        center_y = y + mh // 2
        
        # v7b: GENEROUS ellipse covering cockpit + torso grille + hip/midsection
        # Width: ~65% of mech width (increased from 50%)
        # Height: ~55% of mech height (increased from 40%)
        # Position: centered vertically (covers from upper cockpit to hip junction)
        ellipse_w = int(mw * 0.65)
        ellipse_h = int(mh * 0.55)
        ellipse_center_x = center_x
        ellipse_center_y = center_y  # Centered (no offset)
        
        # Draw the protection ellipse INTO protect_mask_uint (modified in-place)
        cv2.ellipse(protect_mask_uint, 
                    (ellipse_center_x, ellipse_center_y),
                    (ellipse_w // 2, ellipse_h // 2),
                    0, 0, 360, 255, -1)
        
        # Save mask parameters for diagnostics and documentation
        protect_mask_params = {
            "type": "ellipse",
            "center": {"x": int(ellipse_center_x), "y": int(ellipse_center_y)},
            "radii": {"width": int(ellipse_w), "height": int(ellipse_h)},
            "coverage": {"width_pct": 65, "height_pct": 55}
        }
    
    # Convert to boolean for use in protection logic
    protect_mask = protect_mask_uint > 0
    
    # Step 5: Remove dark internal regions (blueprint artifacts in arm cavities)
    # v2 LOGIC: remove dark + low-saturation regions (gray blueprint lines)
    # but keep dark + colored regions (actual dark mech parts)
    # v6 ADDITION: do NOT remove anything inside the torso protection mask
    hsv = cv2.cvtColor(rgb, cv2.COLOR_BGR2HSV)
    
    # Blueprint artifacts are dark AND desaturated (gray schematic lines)
    internal_dark = (gray < 35) & (mask_uint > 0)
    internal_gray = (hsv[:, :, 1] < 40) & (mask_uint > 0)  # Low saturation
    
    # Only remove pixels that are BOTH dark and gray (blueprint)
    blueprint_artifacts = internal_dark & internal_gray
    
    # EXCLUDE protected torso pixels
    blueprint_artifacts = blueprint_artifacts & (~protect_mask)
    
    # Only remove if they form coherent cavity regions (not just edges)
    kernel_cavity = np.ones((5, 5), np.uint8)
    dark_cavities = cv2.morphologyEx(blueprint_artifacts.astype(np.uint8) * 255, 
                                      cv2.MORPH_OPEN, kernel_cavity, iterations=1)
    
    # Remove cavities from mask
    mask_uint[dark_cavities > 0] = 0
    
    # Step 6: Reduce fringing
    # Slightly erode to remove dark outline pixels
    kernel_tiny = np.ones((2, 2), np.uint8)
    mask_uint = cv2.erode(mask_uint, kernel_tiny, iterations=1)
    
    # Step 7: Smooth alpha edges
    mask_float = mask_uint.astype(np.float32) / 255.0
    mask_smooth = cv2.GaussianBlur(mask_float, (5, 5), 1.0)
    mask_final = (mask_smooth * 255).astype(np.uint8)
    
    # Step 8: Create RGBA output
    result = np.zeros((h, w, 4), dtype=np.uint8)
    result[:, :, :3] = rgb
    result[:, :, 3] = mask_final
    
    return result, mask_final, protect_mask_uint, protect_mask_params


def find_content_bounds(mask):
    """Find the bounding box of non-zero pixels in the mask."""
    coords = cv2.findNonZero(mask)
    if coords is None:
        return 0, 0, mask.shape[1], mask.shape[0]
    x, y, w, h = cv2.boundingRect(coords)
    return x, y, w, h


def crop_and_scale_to_match(capture_rgba, ref_rgba, protect_mask_params, scale_adjustment=1.0):
    """
    Iteratively find best crop, scale, and position to match reference.
    
    Strategy:
    1. Find the mech bounds in capture
    2. Scale to roughly match the reference height
    3. Center on the canvas, preferring foot alignment at the bottom
    
    Args:
        scale_adjustment: Fine-tuning multiplier for the calculated scale (default 1.0)
    
    Returns: (matted_result, params_dict)
    """
    # Get masks
    cap_mask = capture_rgba[:, :, 3]
    ref_mask = ref_rgba[:, :, 3]
    
    # Find bounds
    cap_x, cap_y, cap_w, cap_h = find_content_bounds(cap_mask)
    ref_x, ref_y, ref_w, ref_h = find_content_bounds(ref_mask)
    
    print(f"Capture bounds: x={cap_x}, y={cap_y}, w={cap_w}, h={cap_h}")
    print(f"Reference bounds: x={ref_x}, y={ref_y}, w={ref_w}, h={ref_h}")
    
    # Crop to content
    cropped = capture_rgba[cap_y:cap_y+cap_h, cap_x:cap_x+cap_w]
    
    # Calculate scale to match reference height
    # Apply adjustment factor for fine-tuning
    scale = (ref_h / cap_h) * scale_adjustment
    
    print(f"Calculated scale: {scale:.3f} (adjustment: {scale_adjustment})")
    
    # Scale the cropped mech
    new_w = int(cap_w * scale)
    new_h = int(cap_h * scale)
    scaled = cv2.resize(cropped, (new_w, new_h), interpolation=cv2.INTER_LANCZOS4)
    
    # Create canvas
    canvas = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH, 4), dtype=np.uint8)
    
    # Position: center horizontally, align feet at bottom
    ref_bottom_margin = CANVAS_HEIGHT - (ref_y + ref_h)
    print(f"Reference bottom margin: {ref_bottom_margin}px")
    
    # Place the scaled mech
    # X: centered
    dest_x = (CANVAS_WIDTH - new_w) // 2
    # Y: align bottom with reference bottom
    dest_y = CANVAS_HEIGHT - new_h - ref_bottom_margin
    
    # Clamp to canvas
    if dest_x < 0:
        scaled = scaled[:, -dest_x:]
        new_w = scaled.shape[1]
        dest_x = 0
    if dest_y < 0:
        scaled = scaled[-dest_y:, :]
        new_h = scaled.shape[0]
        dest_y = 0
    
    # Crop if too large
    if dest_x + new_w > CANVAS_WIDTH:
        new_w = CANVAS_WIDTH - dest_x
        scaled = scaled[:, :new_w]
    if dest_y + new_h > CANVAS_HEIGHT:
        new_h = CANVAS_HEIGHT - dest_y
        scaled = scaled[:new_h, :]
    
    # Paste onto canvas
    canvas[dest_y:dest_y+new_h, dest_x:dest_x+new_w] = scaled
    
    # Build parameters dictionary
    params = {
        "source": {
            "file": str(CAPTURE_FRAME),
            "dimensions": {
                "width": capture_rgba.shape[1],
                "height": capture_rgba.shape[0]
            }
        },
        "crop": {
            "x": int(cap_x),
            "y": int(cap_y),
            "width": int(cap_w),
            "height": int(cap_h)
        },
        "scale": float(scale),
        "scale_adjustment": float(scale_adjustment),
        "destination": {
            "x": int(dest_x),
            "y": int(dest_y),
            "canvas_width": CANVAS_WIDTH,
            "canvas_height": CANVAS_HEIGHT
        },
        "reference": {
            "file": str(PACK_IDLE),
            "content_bounds": {
                "x": int(ref_x),
                "y": int(ref_y),
                "width": int(ref_w),
                "height": int(ref_h)
            },
            "bottom_margin": int(ref_bottom_margin)
        },
        "processing": {
            "background_removal": "v7b: v2 cavity removal (dark + desaturated) + enlarged torso protection mask (65%×55% ellipse) - FIXED mask creation bug",
            "torso_protection": protect_mask_params,
            "interpolation": "LANCZOS4",
            "alignment_strategy": "feet at bottom, centered horizontally"
        }
    }
    
    return canvas, params


def create_diagnostics(matted, reference, params):
    """Generate visual diagnostics showing the match quality."""
    
    # 1. Side-by-side comparison
    sidebyside = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH * 3 + 20, 4), dtype=np.uint8)
    sidebyside[:, :CANVAS_WIDTH] = reference
    sidebyside[:, CANVAS_WIDTH+10:CANVAS_WIDTH*2+10] = matted
    
    # Difference (overlay)
    overlay = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH, 4), dtype=np.uint8)
    # Reference in green
    overlay[:, :, 1] = reference[:, :, 3]
    # Matted in red
    overlay[:, :, 2] = matted[:, :, 3]
    # Overlap appears yellow
    overlay[:, :, 3] = np.maximum(reference[:, :, 3], matted[:, :, 3])
    
    sidebyside[:, CANVAS_WIDTH*2+20:] = overlay
    
    # Add checkerboard behind everything for visibility
    checker_size = 8
    checker = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH * 3 + 20, 3), dtype=np.uint8)
    for i in range(0, CANVAS_HEIGHT, checker_size):
        for j in range(0, CANVAS_WIDTH * 3 + 20, checker_size):
            if ((i // checker_size) + (j // checker_size)) % 2:
                checker[i:i+checker_size, j:j+checker_size] = [64, 64, 64]
            else:
                checker[i:i+checker_size, j:j+checker_size] = [32, 32, 32]
    
    # Composite onto checker
    final = checker.copy()
    for i in range(3):
        alpha = sidebyside[:, :, 3:4] / 255.0
        final[:, :, i] = (sidebyside[:, :, i] * alpha[:, :, 0] + 
                          final[:, :, i] * (1 - alpha[:, :, 0]))
    
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-sidebyside.png"), final)
    
    # 2. Silhouette overlay (reference edges + matted edges)
    ref_edges = cv2.Canny(reference[:, :, 3], 50, 150)
    mat_edges = cv2.Canny(matted[:, :, 3], 50, 150)
    
    edges_img = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH, 3), dtype=np.uint8)
    edges_img[ref_edges > 0] = [0, 255, 0]  # Green for reference
    edges_img[mat_edges > 0] = [0, 0, 255]  # Red for matted
    # Overlap appears as combined color
    
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-edges.png"), edges_img)
    
    # 3. Composite diagnostic (side-by-side + edges in a 2x2 grid)
    composite = np.zeros((CANVAS_HEIGHT * 2 + 10, CANVAS_WIDTH * 2 + 10, 3), dtype=np.uint8)
    
    # Top row: reference | matted (on checker)
    checker_single = np.zeros((CANVAS_HEIGHT, CANVAS_WIDTH, 3), dtype=np.uint8)
    for i in range(0, CANVAS_HEIGHT, checker_size):
        for j in range(0, CANVAS_WIDTH, checker_size):
            if ((i // checker_size) + (j // checker_size)) % 2:
                checker_single[i:i+checker_size, j:j+checker_size] = [64, 64, 64]
            else:
                checker_single[i:i+checker_size, j:j+checker_size] = [32, 32, 32]
    
    ref_viz = checker_single.copy()
    for i in range(3):
        alpha = reference[:, :, 3:4] / 255.0
        ref_viz[:, :, i] = (reference[:, :, i] * alpha[:, :, 0] + 
                            ref_viz[:, :, i] * (1 - alpha[:, :, 0]))
    
    mat_viz = checker_single.copy()
    for i in range(3):
        alpha = matted[:, :, 3:4] / 255.0
        mat_viz[:, :, i] = (matted[:, :, i] * alpha[:, :, 0] + 
                            mat_viz[:, :, i] * (1 - alpha[:, :, 0]))
    
    composite[0:CANVAS_HEIGHT, 0:CANVAS_WIDTH] = ref_viz
    composite[0:CANVAS_HEIGHT, CANVAS_WIDTH+10:CANVAS_WIDTH*2+10] = mat_viz
    
    # Bottom row: overlay | edges
    overlay_viz = checker_single.copy()
    for i in range(3):
        alpha = overlay[:, :, 3:4] / 255.0
        overlay_viz[:, :, i] = (overlay[:, :, i] * alpha[:, :, 0] + 
                                 overlay_viz[:, :, i] * (1 - alpha[:, :, 0]))
    
    composite[CANVAS_HEIGHT+10:CANVAS_HEIGHT*2+10, 0:CANVAS_WIDTH] = overlay_viz
    composite[CANVAS_HEIGHT+10:CANVAS_HEIGHT*2+10, CANVAS_WIDTH+10:CANVAS_WIDTH*2+10] = edges_img
    
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-composite.png"), composite)
    
    # 4. Individual outputs for inspection
    # Matted on checker
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-matted.png"), mat_viz)
    
    # Reference on checker
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-reference.png"), ref_viz)
    
    print("\nDiagnostics saved:")
    print(f"  - diagnostic-composite.png (2×2 grid: ref | matted / overlay | edges)")
    print(f"  - diagnostic-sidebyside.png (ref | matted | overlay)")
    print(f"  - diagnostic-edges.png (edge overlay)")
    print(f"  - diagnostic-matted.png (matted result)")
    print(f"  - diagnostic-reference.png (pack reference)")
    print(f"  - diagnostic-torso-mask.png (yellow overlay shows torso protection zone)")


def main():
    print("=== Timber Wolf Garrison Frame 0 Lock v7b ===\n")
    
    # Load images
    print("Loading images...")
    capture = cv2.imread(str(CAPTURE_FRAME), cv2.IMREAD_UNCHANGED)
    reference = cv2.imread(str(PACK_IDLE), cv2.IMREAD_UNCHANGED)
    
    if capture is None or reference is None:
        print("Error: Could not load images")
        return 1
    
    print(f"Capture: {capture.shape[1]}×{capture.shape[0]}")
    print(f"Reference: {reference.shape[1]}×{reference.shape[0]}\n")
    
    # Remove background
    print("Removing background from capture (v7b: v2 + enlarged torso protection, FIXED)...")
    capture_rgba, mask, protect_mask_uint, protect_mask_params = remove_background(capture)
    
    # Save the cleaned capture for inspection
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-capture-cleaned.png"), capture_rgba)
    print(f"Saved cleaned capture to diagnostic-capture-cleaned.png\n")
    
    # Create torso mask diagnostic overlay
    print("Creating torso protection mask diagnostic...")
    mask_overlay = capture.copy()
    if len(mask_overlay.shape) == 2:
        mask_overlay = cv2.cvtColor(mask_overlay, cv2.COLOR_GRAY2BGR)
    elif mask_overlay.shape[2] == 4:
        mask_overlay = mask_overlay[:, :, :3]
    
    # Overlay the protection mask in semi-transparent yellow (cyan was hard to see)
    protect_pixels = protect_mask_uint > 0
    mask_overlay[protect_pixels] = (mask_overlay[protect_pixels] * 0.5 + 
                                     np.array([0, 255, 255], dtype=np.uint8) * 0.5).astype(np.uint8)
    cv2.imwrite(str(OUTPUT_DIR / "diagnostic-torso-mask.png"), mask_overlay)
    
    protected_pixel_count = np.sum(protect_mask_uint > 0)
    print(f"Saved torso protection mask overlay to diagnostic-torso-mask.png")
    print(f"  Protected pixels: {protected_pixel_count}\n")
    
    # Find best match
    print("Finding optimal crop/scale/position...")
    matted, params = crop_and_scale_to_match(capture_rgba, reference, protect_mask_params)
    
    # Save lock parameters
    print(f"\nSaving lock parameters to {LOCK_JSON.name}...")
    with open(LOCK_JSON, 'w') as f:
        json.dump(params, f, indent=2)
    
    print("\nLock parameters:")
    print(f"  Crop: ({params['crop']['x']}, {params['crop']['y']}) "
          f"{params['crop']['width']}×{params['crop']['height']}")
    print(f"  Scale: {params['scale']:.3f}")
    print(f"  Position: ({params['destination']['x']}, {params['destination']['y']})")
    
    # Generate diagnostics
    print("\nGenerating diagnostics...")
    create_diagnostics(matted, reference, params)
    
    # Calculate match metrics
    ref_mask = reference[:, :, 3]
    mat_mask = matted[:, :, 3]
    
    intersection = np.sum((ref_mask > 0) & (mat_mask > 0))
    union = np.sum((ref_mask > 0) | (mat_mask > 0))
    iou = intersection / union if union > 0 else 0
    
    ref_pixels = np.sum(ref_mask > 0)
    mat_pixels = np.sum(mat_mask > 0)
    
    print(f"\nMatch metrics:")
    print(f"  IoU (Intersection over Union): {iou:.3f}")
    print(f"  Reference pixels: {ref_pixels}")
    print(f"  Matted pixels: {mat_pixels}")
    print(f"  Pixel ratio: {mat_pixels/ref_pixels:.3f}")
    
    print("\n✓ Frame 0 lock v7b complete!")
    print(f"  Lock params: {LOCK_JSON.name}")
    print(f"  Diagnostics: diagnostic-*.png")
    print(f"  Torso mask: diagnostic-torso-mask.png (yellow overlay shows protected region)")
    
    return 0


if __name__ == "__main__":
    exit(main())

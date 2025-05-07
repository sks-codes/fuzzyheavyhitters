import csv

import pandas as pd
import matplotlib.pyplot as plt
import contextily as ctx
import numpy as np
import os
from matplotlib.colors import LogNorm

# Configuration
DATA_FILE = "../data/RideAustin_Weather.csv"  # Update path
SAMPLE_SIZE = 100000  # Adjust as needed
OUTPUT_DIR = "../data/ride_plots"

def load_data():
  """Load and filter rides to Austin area only"""
  print(f"Loading and sampling {SAMPLE_SIZE} rides...")
  df = pd.read_csv(DATA_FILE)

  # Convert timestamps
  df['started_on'] = pd.to_datetime(df['started_on'])
  df['completed_on'] = pd.to_datetime(df['completed_on'])
  df['duration_min'] = (df['completed_on'] - df['started_on']).dt.total_seconds() / 60

  # Austin bounding box (approximate 50km radius)
  AUSTIN_CENTER = (30.2672, -97.7431)  # (lat, lon)
  BUFFER_DEGREES = 1

  # Filter coordinates
  austin_mask = (
      df['start_location_lat'].between(AUSTIN_CENTER[0] - BUFFER_DEGREES,
                                       AUSTIN_CENTER[0] + BUFFER_DEGREES) &
      df['start_location_long'].between(AUSTIN_CENTER[1] - BUFFER_DEGREES,
                                        AUSTIN_CENTER[1] + BUFFER_DEGREES)
  )

  print(f"Filtering: {len(df) - austin_mask.sum()} rides outside Austin area removed")
  df = df[austin_mask]

  # Sample if needed
  return df.sample(min(SAMPLE_SIZE, len(df)))

def visualize_rides(df):
  """Generate precise ride visualizations"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)

  # Calculate dynamic bounds with 10% buffer
  min_lon, max_lon = df['start_location_long'].min(), df['start_location_long'].max()
  min_lat, max_lat = df['start_location_lat'].min(), df['start_location_lat'].max()
  lon_buffer = (max_lon - min_lon) * 0.1
  lat_buffer = (max_lat - min_lat) * 0.1
  extent = (min_lon - lon_buffer, max_lon + lon_buffer,
            min_lat - lat_buffer, max_lat + lat_buffer)

  # 1. Start Locations Heatmap
  plt.figure(figsize=(16, 10))
  hb = plt.hexbin(
      x=df['start_location_long'],
      y=df['start_location_lat'],
      gridsize=100,
      bins='log',
      cmap='inferno',
      mincnt=1,
      extent=extent
  )
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
  plt.colorbar(hb, label='Log10(Ride Count)')
  plt.title('Ride Start Locations')
  plt.savefig(f"{OUTPUT_DIR}/start_locations.png", dpi=150)
  plt.close()

  # 2. Top Routes (sampled for clarity)
  plt.figure(figsize=(16, 10))
  sample_size = min(1000, len(df))
  for _, row in df.sample(sample_size).iterrows():
    plt.plot(
        [row['start_location_long'], row['end_location_long']],
        [row['start_location_lat'], row['end_location_lat']],
        linewidth=0.5,
        alpha=0.2,
        color='red'
    )
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.CartoDB.Positron)
  plt.title(f'Top Ride Routes (Sample of {sample_size})')
  plt.savefig(f"{OUTPUT_DIR}/ride_routes.png", dpi=150)
  plt.close()

  # 3. Speed Analysis
  plt.figure(figsize=(12, 8))
  df['speed_mph'] = (df['distance_travelled'] / 1609.34) / (df['duration_min'] / 60)  # Convert to mph

  valid_speeds = df[(df['speed_mph'] > 1) & (df['speed_mph'] < 100)]  # Filter outliers

  plt.hist2d(
      valid_speeds['distance_travelled'] / 1000,  # Convert to km
      valid_speeds['speed_mph'],
      bins=50,
      cmap='viridis',
      norm=LogNorm()
  )
  plt.colorbar(label='Log10(Ride Count)')
  plt.xlabel('Distance (km)')
  plt.ylabel('Speed (mph)')
  plt.title('Ride Speed Distribution')
  plt.savefig(f"{OUTPUT_DIR}/speed_analysis.png", dpi=150)
  plt.close()

def clear_csv_keep_header(input_file, output_file=None):
  """Keep only the header row of a CSV file"""
  if output_file is None:
    output_file = input_file  # Overwrite original file

  with open(input_file, 'r') as f:
    reader = csv.reader(f)
    header = next(reader)  # Read just the first line

  with open(output_file, 'w', newline='') as f:
    writer = csv.writer(f)
    writer.writerow(header)

def visualize_rides_with_heavy_hitters(df, heavy_hitters_path):
  """Generate heatmap with heavy hitters overlaid"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)

  # Load heavy hitters data
  try:
    heavy_hitters = pd.read_csv(heavy_hitters_path)
    print(f"Loaded {len(heavy_hitters)} heavy hitters")
  except Exception as e:
    print(f"Error loading heavy hitters: {e}")
    return

  # Calculate dynamic bounds with 10% buffer
  min_lon, max_lon = df['start_location_long'].min(), df['start_location_long'].max()
  min_lat, max_lat = df['start_location_lat'].min(), df['start_location_lat'].max()
  lon_buffer = (max_lon - min_lon) * 0.1
  lat_buffer = (max_lat - min_lat) * 0.1
  extent = (min_lon - lon_buffer, max_lon + lon_buffer,
            min_lat - lat_buffer, max_lat + lat_buffer)

  # Create figure
  plt.figure(figsize=(16, 10))

  # 1. Create heatmap
  hb = plt.hexbin(
      x=df['start_location_long'],
      y=df['start_location_lat'],
      gridsize=100,
      bins='log',
      cmap='inferno',
      mincnt=1,
      extent=extent,
      alpha=0.7
  )

  # 2. Overlay heavy hitters
  plt.scatter(
      heavy_hitters['longitude'],
      heavy_hitters['latitude'],
      c='cyan',
      edgecolors='black',
      s=100,  # Marker size
      label='Heavy Hitters',
      alpha=0.9,
      marker='*'
  )

  # Add map background
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)

  # Add colorbar and legend
  plt.colorbar(hb, label='Log10(Ride Count)')
  plt.legend()

  # Customize plot
  plt.title('Ride Start Locations with Heavy Hitters Overlay')
  plt.xlabel('Longitude')
  plt.ylabel('Latitude')

  # Save figure
  output_path = f"{OUTPUT_DIR}/heatmap_with_heavy_hitters.png"
  plt.savefig(output_path, dpi=150, bbox_inches='tight')
  plt.close()

  print(f"Heavy hitters overlay saved to {output_path}")

def main():
  df = load_data()
  visualize_rides(df)

  # Add visualization with heavy hitters overlay
  heavy_hitters_path = "../data/ride_heavy_hitters.csv"  # Update path
  if os.path.exists(heavy_hitters_path):
    visualize_rides_with_heavy_hitters(df, heavy_hitters_path)
  else:
    print(f"Heavy hitters file not found at {heavy_hitters_path}")

  print(f"Visualizations saved to {OUTPUT_DIR}/")
  clear_csv_keep_header(heavy_hitters_path)

if __name__ == "__main__":
  main()
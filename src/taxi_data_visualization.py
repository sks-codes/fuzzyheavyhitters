import pandas as pd
import matplotlib.pyplot as plt
import contextily as ctx
import numpy as np
from shapely.geometry import shape
import geopandas as gpd
import json
import os
from matplotlib.colors import LogNorm

# Configuration
TAXI_FILE = "../data/yellow_tripdata_2025-01.parquet"
ZONES_FILE = "../data/taxi_zones.geojson"
SAMPLE_SIZE = 100000
OUTPUT_DIR = "../data/taxi_plots"
L_INF_EPSILON = 0.01  # ~1km fuzzy radius in degrees


def load_zones():
  """Load taxi zone centroids with robust ID handling"""
  with open(ZONES_FILE) as f:
    zones_data = json.load(f)

  zones = []
  for feature in zones_data['features']:
    try:
      zone_id = str(feature['properties']['location_id'])  # Ensure string type
      geom = shape(feature['geometry'])
      centroid = geom.centroid
      zones.append({
        'LocationID': zone_id,
        'longitude': centroid.x,
        'latitude': centroid.y
      })
    except (KeyError, AttributeError) as e:
      print(f"Skipping zone with invalid data: {e}")
      continue

  return pd.DataFrame(zones)

def process_taxi_data(taxi_df, zones_df):
  """Merge taxi data with zone coordinates with validation"""
  # Convert IDs to consistent string format
  taxi_df['PULocationID'] = taxi_df['PULocationID'].astype(str).str.strip()
  taxi_df['DOLocationID'] = taxi_df['DOLocationID'].astype(str).str.strip()
  zones_df['LocationID'] = zones_df['LocationID'].astype(str).str.strip()

  # Diagnostic output
  print("\nZone ID Validation:")
  print(f"Taxi PULocationID sample: {taxi_df['PULocationID'].unique()[:5]}")
  print(f"Zones LocationID sample: {zones_df['LocationID'].unique()[:5]}")

  # Check for non-matching IDs
  unique_pu_ids = set(taxi_df['PULocationID'].unique())
  missing_pu = unique_pu_ids - set(zones_df['LocationID'])
  print(f"\n{len(missing_pu)} pickup locations missing from zones file")
  if missing_pu:
    print("Sample missing PULocationIDs:", list(missing_pu)[:5])

  # Merge pickup locations (inner join to only keep matched records)
  pu_merged = pd.merge(
      taxi_df[['PULocationID']],
      zones_df,
      left_on='PULocationID',
      right_on='LocationID',
      how='inner'
  ).rename(columns={
    'longitude': 'pu_longitude',
    'latitude': 'pu_latitude'
  })

  # Merge dropoff locations
  do_merged = pd.merge(
      taxi_df[['DOLocationID']],
      zones_df,
      left_on='DOLocationID',
      right_on='LocationID',
      how='inner'
  ).rename(columns={
    'longitude': 'do_longitude',
    'latitude': 'do_latitude'
  })

  # Combine results
  result = taxi_df.copy()
  result[['pu_longitude', 'pu_latitude']] = pu_merged[['pu_longitude', 'pu_latitude']]
  result[['do_longitude', 'do_latitude']] = do_merged[['do_longitude', 'do_latitude']]

  # Drop rows with missing coordinates
  result = result.dropna(subset=['pu_latitude', 'pu_longitude', 'do_latitude', 'do_longitude'])

  print(f"\nGeocoding results: {len(result)}/{len(taxi_df)} records matched ({len(result)/len(taxi_df)*100:.1f}%)")
  return result

def add_fuzziness(df):
  """Add L∞-ball fuzzy locations"""
  # Pickup locations
  df['pu_longitude_fuzzy'] = df['pu_longitude'] + np.random.uniform(-L_INF_EPSILON, L_INF_EPSILON, len(df))
  df['pu_latitude_fuzzy'] = df['pu_latitude'] + np.random.uniform(-L_INF_EPSILON, L_INF_EPSILON, len(df))

  # Dropoff locations
  df['do_longitude_fuzzy'] = df['do_longitude'] + np.random.uniform(-L_INF_EPSILON, L_INF_EPSILON, len(df))
  df['do_latitude_fuzzy'] = df['do_latitude'] + np.random.uniform(-L_INF_EPSILON, L_INF_EPSILON, len(df))

  return df

def visualize_heatmaps(df):
  """Generate fuzzy and exact heatmaps"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)

  # 1. Exact Pickup Locations
  plt.figure(figsize=(16, 10))
  hb = plt.hexbin(
      x=df['pu_longitude'],
      y=df['pu_latitude'],
      gridsize=100,
      bins='log',
      cmap='inferno',
      mincnt=1,
      extent=(-74.3, -73.7, 40.5, 40.9)  # NYC bounds
  )
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
  plt.colorbar(hb, label='Log10(Trip Count)')
  plt.title('Exact Pickup Locations')
  plt.savefig(f"{OUTPUT_DIR}/exact_pickups.png", dpi=150)
  plt.close()

  # 2. Fuzzy Pickup Locations (L∞-balls)
  plt.figure(figsize=(16, 10))
  hb = plt.hexbin(
      x=df['pu_longitude_fuzzy'],
      y=df['pu_latitude_fuzzy'],
      gridsize=100,
      bins='log',
      cmap='viridis',
      mincnt=1,
      extent=(-74.3, -73.7, 40.5, 40.9)
  )
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
  plt.colorbar(hb, label='Log10(Fuzzy Trip Count)')
  plt.title(f'Fuzzy Pickup Locations (L∞ ε={L_INF_EPSILON}°)')
  plt.savefig(f"{OUTPUT_DIR}/fuzzy_pickups.png", dpi=150)
  plt.close()

  # 3. Top Routes (fuzzy origin-destination pairs)
  top_routes = df.groupby([
    pd.cut(df['pu_longitude_fuzzy'], bins=np.linspace(-74.3, -73.7, 50)),
    pd.cut(df['pu_latitude_fuzzy'], bins=np.linspace(40.5, 40.9, 50)),
    pd.cut(df['do_longitude_fuzzy'], bins=np.linspace(-74.3, -73.7, 50)),
    pd.cut(df['do_latitude_fuzzy'], bins=np.linspace(40.5, 40.9, 50))
  ]).size().nlargest(20).reset_index()
  print("here")

  plt.figure(figsize=(16, 10))
  for _, row in top_routes.iterrows():
    plt.plot(
        [row['pu_longitude_fuzzy'].mid, row['do_longitude_fuzzy'].mid],
        [row['pu_latitude_fuzzy'].mid, row['do_latitude_fuzzy'].mid],
        linewidth=row[0]/top_routes[0].max()*5,
        alpha=0.5,
        color='red'
    )
  ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.CartoDB.Positron)
  plt.title('Top 20 Fuzzy Taxi Routes')
  plt.savefig(f"{OUTPUT_DIR}/fuzzy_routes.png", dpi=150)
  plt.close()

def main():
  print("Loading taxi zones...")
  zones_df = load_zones()

  print(f"Sampling {SAMPLE_SIZE} taxi trips...")
  taxi_df = pd.read_parquet(TAXI_FILE).sample(SAMPLE_SIZE)

  print("Geocoding trip locations...")
  taxi_df = process_taxi_data(taxi_df, zones_df)

  print("Adding L∞ fuzziness...")
  taxi_df = add_fuzziness(taxi_df)

  print("Generating visualizations...")
  visualize_heatmaps(taxi_df)

  print(f"Visualizations saved to {OUTPUT_DIR}/")

if __name__ == "__main__":
  main()
import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import os
from datetime import datetime
import contextily as ctx
from matplotlib.colors import LogNorm

# Configuration
COVID_FILE = "../data/COVID-19_Case_Surveillance_Public_Use_Data_with_Geography_20250430.csv"
CENTROID_FILE = "../data/county_centroids.csv"
SAMPLE_SIZE = 100000
CHUNKSIZE = 50000
OUTPUT_DIR = "../data/covid_plots"
RANDOM_SEED = 42

def load_centroids():
  """Load and validate centroid data"""
  centroids = pd.read_csv(CENTROID_FILE, dtype={'fips_code': str})

  # Ensure FIPS is 5-digit string
  centroids['FIPS'] = centroids['fips_code'].str.zfill(5)

  print(f"Loaded {len(centroids)} county centroids")
  print("Sample of centroids:")
  print(centroids[['FIPS', 'latitude', 'longitude']].head())
  return centroids

def create_sample():
  """Create sampled dataset with validation"""
  try:
    file_size = os.path.getsize(COVID_FILE)
    approx_lines = file_size // 200

    np.random.seed(RANDOM_SEED)
    skip_rows = np.sort(np.random.choice(
        np.arange(1, approx_lines+1),
        size=approx_lines - SAMPLE_SIZE,
        replace=False
    ))

    df = pd.read_csv(
        COVID_FILE,
        skiprows=skip_rows,
        dtype={'county_fips_code': 'string',
               'state_fips_code': 'string',
               'res_state': 'category'},
        on_bad_lines='warn'
    )

    print("\nSample data validation:")
    print(f"Total rows sampled: {len(df)}")
    print("State FIPS codes sample:", df['state_fips_code'].dropna().unique()[:5])
    print("County FIPS codes sample:", df['county_fips_code'].dropna().unique()[:5])

    return df

  except Exception as e:
    print(f"Error in direct sampling: {str(e)}")
    print("Falling back to chunked sampling...")
    return chunked_sample()

def chunked_sample():
  """Chunked sampling with validation"""
  samples = []
  for chunk in pd.read_csv(
      COVID_FILE,
      chunksize=CHUNKSIZE,
      dtype={'county_fips_code': 'string',
             'state_fips_code': 'string'},
      on_bad_lines='warn'
  ):
    # Ensure we have required columns
    if not all(col in chunk.columns for col in ['state_fips_code', 'county_fips_code']):
      print("Missing required columns in chunk")
      continue

    samples.append(chunk.sample(min(SAMPLE_SIZE//10, len(chunk))))
    if len(pd.concat(samples)) >= SAMPLE_SIZE:
      break

  df = pd.concat(samples).sample(SAMPLE_SIZE)
  print("\nChunked sample validation:")
  print(f"Total rows sampled: {len(df)}")
  print("State FIPS codes sample:", df['state_fips_code'].dropna().unique()[:5])
  print("County FIPS codes sample:", df['county_fips_code'].dropna().unique()[:5])
  return df

def add_coordinates(df, centroids):
  """Merge with coordinates using correct FIPS handling"""
  # COVID's county_fips_code is already the full 5-digit FIPS
  df['FIPS'] = df['county_fips_code'].str.zfill(5)

  # Diagnostic output
  print("\nFIPS Code Verification:")
  print("COVID FIPS sample:", df['FIPS'].dropna().unique()[:5])
  print("Centroid FIPS sample:", centroids['FIPS'].dropna().unique()[:5])

  # Merge with centroids
  merged = pd.merge(
      df,
      centroids,
      on='FIPS',
      how='inner'
  )

  print("\nMerge results:")
  print(f"Matched {len(merged)}/{len(df)} records ({len(merged)/len(df)*100:.1f}%)")

  return merged

def generate_visualizations(df):
  """Generate visualizations only if we have data"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)

  if len(df) == 0:
    print("Skipping visualizations - no geocoded data available")
    return

  try:
    # 1. Monthly Cases Trend
    plt.figure(figsize=(12, 6))
    df['case_month'] = pd.to_datetime(df['case_month'])
    monthly = df.set_index('case_month').resample('ME').size()
    if len(monthly) > 0:
      monthly.plot(title='Monthly COVID-19 Cases')
      plt.savefig(f"{OUTPUT_DIR}/monthly_trend.png")
      plt.close()
  except Exception as e:
    print(f"Error generating monthly trend: {str(e)}")

  try:
    # 2. State Distribution
    plt.figure(figsize=(12, 8))
    state_counts = df['res_state'].value_counts().head(20)
    if len(state_counts) > 0:
      state_counts.plot(kind='barh')
      plt.title('Top 20 States by Case Count')
      plt.savefig(f"{OUTPUT_DIR}/state_distribution.png")
      plt.close()
  except Exception as e:
    print(f"Error generating state distribution: {str(e)}")

  try:
    # 3. Age Group Distribution
    plt.figure(figsize=(10, 6))
    age_counts = df['age_group'].value_counts()
    if len(age_counts) > 0:
      age_counts.plot(kind='bar')
      plt.title('Age Group Distribution')
      plt.xticks(rotation=45)
      plt.savefig(f"{OUTPUT_DIR}/age_distribution.png")
      plt.close()
  except Exception as e:
    print(f"Error generating age distribution: {str(e)}")

  try:
    # 4. Spatial Heatmap (Updated with modern basemap)
    plt.figure(figsize=(16, 10))

    # Create heatmap
    hb = plt.hexbin(
        x=df['longitude'],
        y=df['latitude'],
        gridsize=100,
        bins='log',
        cmap='inferno',
        mincnt=1,
        extent=(-125, -65, 24, 50)  # Continental US bounds
    )

    # Add modern basemap (OpenStreetMap)
    ctx.add_basemap(
        plt.gca(),
        crs="EPSG:4326",
        source=ctx.providers.OpenStreetMap.Mapnik,
        attribution_size=6
    )

    plt.colorbar(hb, label='Log10(Case Count)')
    plt.title('COVID-19 Case Density by County')
    plt.savefig(f"{OUTPUT_DIR}/case_density_heatmap.png", dpi=150, bbox_inches='tight')
    plt.close()

  except Exception as e:
    print(f"Error generating heatmap: {str(e)}")
    print("Proceeding without basemap...")

    # Fallback: Heatmap without basemap
    plt.figure(figsize=(16, 10))
    plt.hexbin(
        x=df['longitude'],
        y=df['latitude'],
        gridsize=100,
        bins='log',
        cmap='inferno',
        mincnt=1
    )
    plt.colorbar(label='Log10(Case Count)')
    plt.title('COVID-19 Case Density by County')
    plt.savefig(f"{OUTPUT_DIR}/case_density_heatmap_fallback.png", dpi=150)
    plt.close()

def main():
  print(f"Starting analysis at {datetime.now()}")

  # 1. Load data
  centroids = load_centroids()
  sampled_df = create_sample()

  # 2. Geocode
  print("\nGeocoding sample...")
  geo_df = add_coordinates(sampled_df, centroids)

  # 3. Generate output
  if len(geo_df) > 0:
    print("\nSample of geocoded data:")
    print(geo_df[['res_state', 'county_fips_code', 'latitude', 'longitude']].head())

    print("\nGenerating visualizations...")
    generate_visualizations(geo_df)
  else:
    print("\nNo geocoded data available for visualizations")

  print(f"\nAnalysis completed at {datetime.now()}")

if __name__ == "__main__":
  main()
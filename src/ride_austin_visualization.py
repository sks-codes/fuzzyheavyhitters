import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
import contextily as ctx
from matplotlib.colors import LogNorm
from collections import defaultdict
import numpy as np

import os
from datetime import datetime

# Configuration
DATA_FILE = "../data/Rides_DataA.csv"  # Update with your actual file path
OUTPUT_DIR = "../data/ride_plots"
HEAVY_HITTERS = "../data/ride_heavy_hitters.csv"

def load_and_clean_data():
  """Load data with proper timezone handling to local Austin time"""
  print("Loading and cleaning data...")

  # Load data with explicit datetime parsing
  datetime_cols = ['started_on', 'created_date', 'updated_date',
                   'completed_on', 'driver_reached_on']

  # First load the raw data
  df = pd.read_csv(DATA_FILE)

  # Convert datetime columns to local Austin time (GMT-5)
  for col in datetime_cols:
    if col in df.columns:
      # First ensure we're working with strings
      df[col] = df[col].astype(str)

      # Remove any timezone offset for consistent parsing
      df[col] = df[col].str.replace(r'[-+]\d{2}:\d{2}$', '', regex=True)

      # Parse as naive datetime (will treat as UTC but we'll convert)
      df[col] = pd.to_datetime(df[col], errors='coerce')

      # Localize to UTC first, then convert to Austin time
      df[col] = df[col].dt.tz_localize('UTC').dt.tz_convert('America/Chicago')

      # Convert to naive local time for analysis
      df[col] = df[col].dt.tz_localize(None)

  # Basic data quality checks
  print(f"Original data size: {len(df)} rows")

  # Filter for valid started_on timestamps
  df = df[df['started_on'].notna()]

  # Filter for outliers
  AUSTIN_CENTER = (30.2672, -97.7431)
  BUFFER_DEGREES = 1
  austin_mask = (
      df['start_location_lat'].between(AUSTIN_CENTER[0] - BUFFER_DEGREES,
                                       AUSTIN_CENTER[0] + BUFFER_DEGREES) &
      df['start_location_long'].between(AUSTIN_CENTER[1] - BUFFER_DEGREES,
                                        AUSTIN_CENTER[1] + BUFFER_DEGREES)
  )
  df = df[austin_mask]

  AUSTIN_CENTER = (30.2672, -97.7431)
  BUFFER_DEGREES = 1
  austin_mask = (
      df['start_location_lat'].between(AUSTIN_CENTER[0] - BUFFER_DEGREES,
                                       AUSTIN_CENTER[0] + BUFFER_DEGREES) &
      df['start_location_long'].between(AUSTIN_CENTER[1] - BUFFER_DEGREES,
                                        AUSTIN_CENTER[1] + BUFFER_DEGREES)
  )
  df = df[austin_mask]
  print(f"After removing null started_on timestamps and geographic outliers: {len(df)} rows")

  # Filter for realistic dates (2016-2017)
  start_date = pd.to_datetime('2016-01-01')
  end_date = pd.to_datetime('2018-01-01')
  df = df[df['started_on'].between(start_date, end_date)]
  print(f"After date filtering: {len(df)} rows")

  # Add hour of day column for analysis
  df['hour_of_day'] = df['started_on'].dt.hour

  return df

def plot_daily_rides(df):
  """Average rides per day calculation with new data"""
  # First calculate rides per day
  rides_per_day = df.set_index('started_on').resample('D').size()

  # Then calculate average by day of week
  avg_rides = rides_per_day.groupby(rides_per_day.index.day_name()).mean()
  day_order = ['Monday', 'Tuesday', 'Wednesday', 'Thursday',
               'Friday', 'Saturday', 'Sunday']
  avg_rides = avg_rides.reindex(day_order)

  # Create plot with context
  plt.figure(figsize=(12, 6))

  ax = sns.barplot(x=avg_rides.index, y=avg_rides.values, order=day_order)

  overall_avg = rides_per_day.mean()

  for p in ax.patches:
    ax.annotate(f"{p.get_height():.1f}",
                (p.get_x() + p.get_width() / 2., p.get_height()),
                ha='center', va='center', xytext=(0, 5), textcoords='offset points')

  plt.title('Average Number of Rides per Day of Week')
  plt.xlabel('Day of Week')
  plt.ylabel('Average Number of Rides')
  plt.xticks(rotation=45)
  plt.tight_layout()
  plt.savefig(f"{OUTPUT_DIR}/avg_rides_per_day.png", dpi=150)
  plt.close()

  # Print summary stats
  print("\nDaily Ride Statistics:")
  print(f"Overall average: {overall_avg:.1f} rides per day")
  print(f"Busiest day: {avg_rides.idxmax()} ({avg_rides.max():.1f} rides)")
  print(f"Slowest day: {avg_rides.idxmin()} ({avg_rides.min():.1f} rides)")

def plot_hourly_patterns(df):
  """Plot hourly patterns with proper timezone handling"""
  plt.figure(figsize=(12, 6))

  hourly_counts = df['hour_of_day'].value_counts().sort_index()

  ax = sns.barplot(x=hourly_counts.index, y=hourly_counts.values)

  plt.title('Ride Count by Hour of Day (Local Austin Time)')
  plt.xlabel('Hour of Day (0-23)')
  plt.ylabel('Number of Rides')
  plt.xticks(range(0, 24))
  plt.grid(False)
  plt.savefig(f"{OUTPUT_DIR}/hourly_patterns.png", dpi=150)
  plt.close()


def plot_austin_heatmap(df):
  """Generate precise ride visualizations"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)

  heatmap_columns = ['start', 'end']

  # Calculate dynamic bounds with 10% buffer
  for column in heatmap_columns:
    min_lon, max_lon = df[column+'_location_long'].min(), df[column+'_location_long'].max()
    min_lat, max_lat = df[column+'_location_lat'].min(), df[column+'_location_lat'].max()
    lon_buffer = (max_lon - min_lon) * 0.1
    lat_buffer = (max_lat - min_lat) * 0.1
    extent = (min_lon - lon_buffer, max_lon + lon_buffer,
              min_lat - lat_buffer, max_lat + lat_buffer)

    plt.figure(figsize=(16, 10))

    # Create a pivot table for the heatmap
    heatmap_data = df.groupby([column+'_location_lat', column+'_location_long']).size().reset_index(name='counts')

    # Plot with scatter plot for precise locations
    plt.scatter(
        x=heatmap_data[column+'_location_long'],
        y=heatmap_data[column+'_location_lat'],
        c=heatmap_data['counts'],
        cmap='inferno',
        norm=LogNorm(),
        s=1,
        alpha=0.7
    )

    try:
      heavy_hitters = pd.read_csv(HEAVY_HITTERS)
      print(f"Loaded {len(heavy_hitters)} heavy hitters")
    except Exception as e:
      print(f"Error loading heavy hitters: {e}")
      return

    plt.scatter(
        heavy_hitters['longitude'],
        heavy_hitters['latitude'],
        c='cyan',
        edgecolors='black',
        s=50,
        label='Heavy Hitters',
        alpha=0.9,
        marker='*'
    )

    ctx.add_basemap(plt.gca(), crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
    plt.colorbar(label='Log10(Ride Count)')
    if column == 'start':
      plt.title('Ride Start Locations')
    else:
      plt.title('Ride End Locations')
    plt.xlabel('Longitude')
    plt.ylabel('Latitude')
    plt.savefig(f"{OUTPUT_DIR}/"+column+"_locations.png", dpi=150)
    plt.close()

def calculate_ride_percentages(df, x_values, precision=3):
  """Calculate what % of all rides are in each grid location's neighborhood"""
  # Convert to grid coordinates
  lat_min, lon_min = 30.1672, -98.7431
  df['lat_grid'] = ((df['start_location_lat'] - lat_min) * 10**precision).astype(int)
  df['lon_grid'] = ((df['start_location_long'] - lon_min) * 10**precision).astype(int)

  total_rides = len(df)
  results = {}

  for x in x_values:
    # Dictionary to count rides in each neighborhood
    neighborhood_counts = defaultdict(int)

    for _, row in df.iterrows():
      lat, lon = row['lat_grid'], row['lon_grid']
      # Mark all grid points whose neighborhood contains this ride
      for i in range(lat - x, lat + x + 1):
        for j in range(lon - x, lon + x + 1):
          neighborhood_counts[(i,j)] += 1

    # Convert to sorted list of percentages
    percentages = sorted([(count/total_rides*100) for count in neighborhood_counts.values()], reverse=True)
    results[x] = percentages

  return results

def plot_ride_percentages(results, x_values):
  """Improved visualization of ride percentages"""
  plt.figure(figsize=(12, 8))

  for x in x_values:
    percentages = results[x]
    plt.plot(np.arange(len(percentages)) + 1,
             percentages,
             label=f'x={x} ({(2*x+1)}x{(2*x+1)} neighborhood)',
             alpha=0.7)

  plt.xlabel('Grid Location Rank (by popularity)')
  plt.ylabel('% of Total Rides')
  plt.title('Start Location Ride Concentration in Austin Grid')

  # plt.yscale('log')
  plt.xscale('log')

  plt.grid(True, which='both', linestyle='--', alpha=0.5)
  # plt.yticks([0.0001, 0.001, 0.01, 0.1, 1, 10],
  #            ['0.0001%', '0.001%', '0.01%', '0.1%', '1%', '10%'])

  plt.legend()
  plt.tight_layout()
  plt.savefig(f"{OUTPUT_DIR}/ride_start_percentage_distribution.png", dpi=150, bbox_inches='tight')
  plt.close()

def print_results(results, x_values):
  """Print results in a readable format"""
  for x in x_values:
    percentages = results[x]
    print(f"\nNeighborhood size x={x} ({(2*x+1)}x{(2*x+1)} grid):")
    print("Top 10 Locations by Ride Percentage:")
    for i, pct in enumerate(percentages[:10]):
      print(f"{i+1}. {pct:.4f}% of rides")
    print(f"\nSummary stats for x={x}:")
    print(f"Max: {max(percentages):.4f}%")
    print(f"Min: {min(percentages):.4f}%")
    print(f"Mean: {np.mean(percentages):.4f}%")
    print(f"Locations covering >1% of rides: {sum(1 for p in percentages if p > 1)}")




def main():
  df = load_and_clean_data()  # Your existing function

  # Define the neighborhood sizes to test
  x_values = [1, 2, 3]  # Example values

  # Calculate percentages
  results = calculate_ride_percentages(df, x_values)

  # Plot results
  plot_ride_percentages(results, x_values)

  # Print formatted results
  print_results(results, x_values)


def main():
  df = load_and_clean_data()

  # Add date features after cleaning
  df['date'] = df['started_on'].dt.date
  df['day_of_week'] = df['started_on'].dt.day_name()
  df['hour'] = df['started_on'].dt.hour

  plot_daily_rides(df)
  plot_hourly_patterns(df)
  plot_austin_heatmap(df)

  print("\nData Summary:")
  print(f"Time period: {df['started_on'].min().date()} to {df['started_on'].max().date()}")
  print(f"Total rides after cleaning: {len(df):,}")
  print(f"Average rides per day: {len(df)/df['date'].nunique():.1f}")

  x_values = [1, 2, 3]
  results = calculate_ride_percentages(df, x_values)
  plot_ride_percentages(results, x_values)
  print_results(results, x_values)

if __name__ == "__main__":
  main()
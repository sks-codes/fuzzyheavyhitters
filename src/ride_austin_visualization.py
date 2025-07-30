import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
import contextily as ctx
from matplotlib.colors import LogNorm
from collections import defaultdict
import numpy as np

import os
from datetime import datetime

DATA_FILE = "../data/Rides_DataA.csv"
OUTPUT_DIR = "../data/ride_plots"
HEAVY_HITTERS = "../data/ride_heavy_hitters.csv"
SAMPLE_DAY_CSV = "../data/sample_busiest_day.csv"
SAMPLE_WEEK_CSV = "../data/sample_busiest_week.csv"
SAMPLE_MONTH_CSV = "../data/sample_busiest_month.csv"

def load_and_clean_data():
  """Load data local Austin time"""
  print("Loading and cleaning data...")

  datetime_cols = ['started_on', 'created_date', 'updated_date',
                   'completed_on', 'driver_reached_on']

  df = pd.read_csv(DATA_FILE)

  for col in datetime_cols:
    if col in df.columns:
      df[col] = df[col].astype(str)

      # Remove any timezone offset for consistent parsing
      df[col] = df[col].str.replace(r'[-+]\d{2}:\d{2}$', '', regex=True)

      #Localize timezone
      df[col] = pd.to_datetime(df[col], errors='coerce')
      df[col] = df[col].dt.tz_localize('UTC').dt.tz_convert('America/Chicago')
      df[col] = df[col].dt.tz_localize(None)

  print(f"Original data size: {len(df)} rows")

  #Filter invalid data/outliers
  df = df[df['started_on'].notna()]

  AUSTIN_CENTER = (30.2672, -97.7431)
  BUFFER_DEGREES = 0.5
  austin_mask1 = (
      df['start_location_lat'].between(AUSTIN_CENTER[0] - BUFFER_DEGREES,
                                       AUSTIN_CENTER[0] + BUFFER_DEGREES) &
      df['start_location_long'].between(AUSTIN_CENTER[1] - BUFFER_DEGREES,
                                        AUSTIN_CENTER[1] + BUFFER_DEGREES)
  )
  df = df[austin_mask1]

  austin_mask2 = (
      df['end_location_lat'].between(AUSTIN_CENTER[0] - BUFFER_DEGREES,
                                       AUSTIN_CENTER[0] + BUFFER_DEGREES) &
      df['end_location_long'].between(AUSTIN_CENTER[1] - BUFFER_DEGREES,
                                        AUSTIN_CENTER[1] + BUFFER_DEGREES)
  )
  df = df[austin_mask2]

  print(f"After removing invalid data and outliers: {len(df)} rows")

  # Add hour of day column for analysis
  df['hour_of_day'] = df['started_on'].dt.hour

  return df

def plot_daily_rides(df):
  rides_per_day = df.set_index('started_on').resample('D').size()

  avg_rides = rides_per_day.groupby(rides_per_day.index.day_name()).mean()
  day_order = ['Monday', 'Tuesday', 'Wednesday', 'Thursday',
               'Friday', 'Saturday', 'Sunday']
  avg_rides = avg_rides.reindex(day_order)

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

  print("\nDaily Ride Statistics:")
  print(f"Overall average: {overall_avg:.1f} rides per day")
  print(f"Busiest day: {rides_per_day.idxmax()} ({rides_per_day.max():.1f} rides)")
  print(f"Busiest day of week: {avg_rides.idxmax()} ({avg_rides.max():.1f} rides)")
  print(f"Slowest day: {avg_rides.idxmin()} ({avg_rides.min():.1f} rides)")

def plot_hourly_patterns(df):
  plt.figure(figsize=(12, 6))

  hourly_counts = df['hour_of_day'].value_counts().sort_index()

  sns.barplot(x=hourly_counts.index, y=hourly_counts.values)

  plt.title('Ride Count by Hour of Day (Local Austin Time)')
  plt.xlabel('Hour of Day (0-23)')
  plt.ylabel('Number of Rides')
  plt.xticks(range(0, 24))
  plt.savefig(f"{OUTPUT_DIR}/hourly_patterns.png", dpi=150)
  plt.close()

def plot_austin_heatmap(df, plot_rides=False):
  """Generate precise ride visualizations with proper map bounding"""
  os.makedirs(OUTPUT_DIR, exist_ok=True)
  heatmap_columns = ['start', 'end']

  for column in heatmap_columns:
    min_lon, max_lon = df[column+'_location_long'].min(), df[column+'_location_long'].max()
    min_lat, max_lat = df[column+'_location_lat'].min(), df[column+'_location_lat'].max()
    lon_buffer = (max_lon - min_lon) * 0.05
    lat_buffer = (max_lat - min_lat) * 0.05
    extent = (
      min_lon - lon_buffer, max_lon + lon_buffer,
      min_lat - lat_buffer, max_lat + lat_buffer
    )

    heatmap_data = df.groupby(
        [column+'_location_lat', column+'_location_long']
    ).size().reset_index(name='counts')

    plt.figure(figsize=(16, 10))
    ax = plt.gca()

    scatter = ax.scatter(
        x=heatmap_data[column+'_location_long'],
        y=heatmap_data[column+'_location_lat'],
        c=heatmap_data['counts'],
        cmap='inferno',
        norm=LogNorm(),
        s=1,
        alpha=0.7,
    )

    ax.set_xlim(extent[0], extent[1])
    ax.set_ylim(extent[2], extent[3])

    if plot_rides:
      try:
        heavy_hitters = pd.read_csv(HEAVY_HITTERS)
        print(f"Loaded {len(heavy_hitters)} heavy hitters")
        for _, row in heavy_hitters.iterrows():
          ax.plot(
              [row['start_longitude'], row['end_longitude']],
              [row['start_latitude'], row['end_latitude']],
              color='lime',
              linewidth=0.08,
              alpha=0.8,
              zorder=2,
              label='Heavy Hitter Route'
          )
          ax.scatter(
              x=[row['start_longitude'], row['end_longitude']],
              y=[row['start_latitude'], row['end_latitude']],
              c=['red', 'blue'],
              s=5,
              edgecolors='black',
              linewidths=0.5,
              alpha=0.9,
              zorder=3,
              label=['Heavy Hitter Start', 'Heavy Hitter End']
          )
        print(f"done plotting heavy hitters")
      except Exception as e:
        print(f"Error loading heavy hitters: {e}")
    else:
      try:
        heavy_hitters = pd.read_csv(HEAVY_HITTERS)
        print(f"Loaded {len(heavy_hitters)} heavy hitters")

        ax.scatter(
            heavy_hitters['longitude'],
            heavy_hitters['latitude'],
            c='#00FF00',
            edgecolors='black',
            linewidths=0,
            s=1,
            label='Heavy Hitters',
            alpha=0.9,
            marker='o'
        )
        print(f"done plotting heavy hitters")
      except Exception as e:
        print(f"Error loading heavy hitters: {e}")

    ctx.add_basemap(ax, crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
    plt.colorbar(scatter, label='Log10(Ride Count)')

    if plot_rides:
      title = 'Ride Start Locations (with routes)' if column == 'start' else 'Ride End Locations (with routes)'
    else:
        title = 'Ride Start Locations' if column == 'start' else 'Ride End Locations'
    plt.title(title)
    plt.xlabel('Longitude')
    plt.ylabel('Latitude')

    if plot_rides:
      plt.savefig(f"{OUTPUT_DIR}/{column}_locations_routes.png", dpi=150)
    else:
      plt.savefig(f"{OUTPUT_DIR}/{column}_locations.png", dpi=150)
    plt.close()

# def plot_austin_heatmap(df, plot_rides=False):
#   """Generate hybrid visualization with heatmap and ONLY heavy hitter routes.
#   When plot_rides=True, shows heatmap with ONLY heavy hitter route lines overlaid."""
#
#   os.makedirs(OUTPUT_DIR, exist_ok=True)
#
#   # Calculate the overall bounding box
#   min_lon = min(df['start_location_long'].min(), df['end_location_long'].min())
#   max_lon = max(df['start_location_long'].max(), df['end_location_long'].max())
#   min_lat = min(df['start_location_lat'].min(), df['end_location_lat'].min())
#   max_lat = max(df['start_location_lat'].max(), df['end_location_lat'].max())
#
#   lon_buffer = (max_lon - min_lon) * 0.05
#   lat_buffer = (max_lat - min_lat) * 0.05
#   extent = (
#     min_lon - lon_buffer, max_lon + lon_buffer,
#     min_lat - lat_buffer, max_lat + lat_buffer
#   )
#
#   try:
#     heavy_hitters = pd.read_csv(HEAVY_HITTERS)
#     print(f"Loaded {len(heavy_hitters)} heavy hitters")
#   except Exception as e:
#     print(f"Error loading heavy hitters: {e}")
#     heavy_hitters = None
#
#   plt.figure(figsize=(16, 10))
#   ax = plt.gca()
#
#   # First plot the heatmap of all points (both starts and ends)
#   all_points = pd.concat([
#     df[['start_location_lat', 'start_location_long']].rename(columns={
#       'start_location_lat': 'lat',
#       'start_location_long': 'lon'
#     }),
#     df[['end_location_lat', 'end_location_long']].rename(columns={
#       'end_location_lat': 'lat',
#       'end_location_long': 'lon'
#     })
#   ])
#
#   heatmap_data = all_points.groupby(['lat', 'lon']).size().reset_index(name='counts')
#
#   # Base heatmap (all points)
#   scatter = ax.scatter(
#       x=heatmap_data['lon'],
#       y=heatmap_data['lat'],
#       c=heatmap_data['counts'],
#       cmap='inferno',
#       norm=LogNorm(),
#       s=2,
#       alpha=0.6,
#       label='All Ride Points'
#   )
#
#   if plot_rides and heavy_hitters is not None:
#     # ONLY plot heavy hitter routes if available
#     if all(col in heavy_hitters.columns
#            for col in ['start_latitude', 'start_longitude',
#                        'end_latitude', 'end_longitude']):
#       for _, row in heavy_hitters.iterrows():
#         # Plot the route line
#         ax.plot(
#             [row['start_longitude'], row['end_longitude']],
#             [row['start_latitude'], row['end_latitude']],
#             color='lime',
#             linewidth=1.5,
#             alpha=0.8,
#             zorder=2,
#             label='Heavy Hitter Route'
#         )
#
#         # Plot special markers for endpoints
#         ax.scatter(
#             x=[row['start_longitude'], row['end_longitude']],
#             y=[row['start_latitude'], row['end_latitude']],
#             c=['red', 'blue'],
#             s=30,
#             edgecolors='white',
#             linewidths=0.5,
#             alpha=0.9,
#             zorder=3,
#             label=['Heavy Hitter Start', 'Heavy Hitter End']
#         )
#       title_suffix = "with Heavy Hitter Routes"
#     else:
#       print("Heavy hitters file doesn't contain route data")
#       title_suffix = "Heatmap"
#   else:
#     # Just highlight heavy hitter points if available
#     if heavy_hitters is not None and all(col in heavy_hitters.columns
#                                          for col in ['latitude', 'longitude']):
#       ax.scatter(
#           heavy_hitters['longitude'],
#           heavy_hitters['latitude'],
#           c='#00FF00',
#           edgecolors='black',
#           linewidths=0,
#           s=1,
#           label='Heavy Hitters',
#           alpha=0.9,
#           marker='o'
#       )
#     title_suffix = "Heatmap"
#
#   # Set map extent and add basemap
#   ax.set_xlim(extent[0], extent[1])
#   ax.set_ylim(extent[2], extent[3])
#   ctx.add_basemap(ax, crs="EPSG:4326", source=ctx.providers.OpenStreetMap.Mapnik)
#
#   # Add colorbar and labels
#   plt.colorbar(scatter, label='Log10(Ride Point Frequency)')
#   plt.title(f"Ride Patterns in Austin {title_suffix}")
#   plt.xlabel('Longitude')
#   plt.ylabel('Latitude')
#
#   # Handle duplicate labels in legend
#   handles, labels = plt.gca().get_legend_handles_labels()
#   by_label = dict(zip(labels, handles))  # Remove duplicates
#   plt.legend(by_label.values(), by_label.keys(), loc='upper right')
#
#   output_filename = "heavy_hitter_routes.png" if (plot_rides and heavy_hitters is not None) else "ride_points_heatmap.png"
#   plt.savefig(f"{OUTPUT_DIR}/{output_filename}", dpi=150, bbox_inches='tight')
#   plt.close()

# def calculate_ride_percentages(df, x_values, precision=3):
#   """Calculate what % of all rides are in each grid location's neighborhood"""
#   # Convert to grid coordinates
#   lat_min, lon_min = 30.1672, -98.7431
#   df['lat_grid'] = ((df['start_location_lat'] - lat_min) * 10**precision).astype(int)
#   df['lon_grid'] = ((df['start_location_long'] - lon_min) * 10**precision).astype(int)
#
#   total_rides = len(df)
#   results = {}
#
#   for x in x_values:
#     # Dictionary to count rides in each neighborhood
#     neighborhood_counts = defaultdict(int)
#
#     for _, row in df.iterrows():
#       lat, lon = row['lat_grid'], row['lon_grid']
#       # Mark all grid points whose neighborhood contains this ride
#       for i in range(lat - x, lat + x + 1):
#         for j in range(lon - x, lon + x + 1):
#           neighborhood_counts[(i,j)] += 1
#
#     # Convert to sorted list of percentages
#     percentages = sorted([(count/total_rides*100) for count in neighborhood_counts.values()], reverse=True)
#     results[x] = percentages
#
#   return results
#
# def plot_ride_percentages(results, x_values):
#   """Improved visualization of ride percentages"""
#   plt.figure(figsize=(12, 8))
#
#   for x in x_values:
#     percentages = results[x]
#     plt.plot(np.arange(len(percentages)) + 1,
#              percentages,
#              label=f'x={x} ({(2*x+1)}x{(2*x+1)} neighborhood)',
#              alpha=0.7)
#
#   plt.xlabel('Grid Location Rank (by popularity)')
#   plt.ylabel('% of Total Rides')
#   plt.title('Start Location Ride Concentration in Austin Grid')
#
#   # plt.yscale('log')
#   plt.xscale('log')
#
#   plt.grid(True, which='both', linestyle='--', alpha=0.5)
#   # plt.yticks([0.0001, 0.001, 0.01, 0.1, 1, 10],
#   #            ['0.0001%', '0.001%', '0.01%', '0.1%', '1%', '10%'])
#
#   plt.legend()
#   plt.tight_layout()
#   plt.savefig(f"{OUTPUT_DIR}/ride_start_percentage_distribution.png", dpi=150, bbox_inches='tight')
#   plt.close()
#
# def print_results(results, x_values):
#   """Print results in a readable format"""
#   for x in x_values:
#     percentages = results[x]
#     print(f"\nNeighborhood size x={x} ({(2*x+1)}x{(2*x+1)} grid):")
#     print("Top 10 Locations by Ride Percentage:")
#     for i, pct in enumerate(percentages[:10]):
#       print(f"{i+1}. {pct:.4f}% of rides")
#     print(f"\nSummary stats for x={x}:")
#     print(f"Max: {max(percentages):.4f}%")
#     print(f"Min: {min(percentages):.4f}%")
#     print(f"Mean: {np.mean(percentages):.4f}%")
#     print(f"Locations covering >1% of rides: {sum(1 for p in percentages if p > 1)}")


def calculate_location_percentages(df, x_values, precision=3, location_type='start'):
  """Calculate what % of all rides are in each grid location's neighborhood"""
  # Select appropriate columns
  lat_col = f'{location_type}_location_lat'
  lon_col = f'{location_type}_location_long'

  # Convert to grid coordinates
  lat_min, lon_min = 29.7672, -98.2431
  df['lat_grid'] = ((df[lat_col] - lat_min) * 10**precision).astype(int)
  df['lon_grid'] = ((df[lon_col] - lon_min) * 10**precision).astype(int)

  total_rides = len(df)
  print(total_rides)
  results = {}

  for x in x_values:
    neighborhood_counts = defaultdict(int)

    for _, row in df.iterrows():
      lat, lon = row['lat_grid'], row['lon_grid']
      for i in range(lat - x, lat + x + 1):
        for j in range(lon - x, lon + x + 1):
          neighborhood_counts[(i,j)] += 1
    percentages = sorted([(count/total_rides*100) for count in neighborhood_counts.values()], reverse=True)
    print(percentages[0])
    print(f"x = {x}, top 10 points = {percentages[10]}")
    print(f"x = {x}, top 100 points = {percentages[100]}")
    print(f"x = {x}, top 1000 points = {percentages[1000]}")
    print(f"x = {x}, top 5000 points = {percentages[5000]}")
    results[x] = percentages

  return results

def calculate_route_percentages(df, x_values, precision=3):
  """Calculate most common start-end neighborhood pairs"""
  # Convert to grid coordinates
  lat_min, lon_min = 30.1672, -98.7431
  df['start_lat_grid'] = ((df['start_location_lat'] - lat_min) * 10**precision).astype(int)
  df['start_lon_grid'] = ((df['start_location_long'] - lon_min) * 10**precision).astype(int)
  df['end_lat_grid'] = ((df['end_location_lat'] - lat_min) * 10**precision).astype(int)
  df['end_lon_grid'] = ((df['end_location_long'] - lon_min) * 10**precision).astype(int)

  total_rides = len(df)
  route_results = {}

  for x in x_values:
    route_counts = defaultdict(int)

    for _, row in df.iterrows():
      # Get neighborhood bounds for start and end
      start_lat, start_lon = row['start_lat_grid'], row['start_lon_grid']
      end_lat, end_lon = row['end_lat_grid'], row['end_lon_grid']

      # Find all start-end neighborhood pairs
      for i in range(start_lat - x, start_lat + x + 1):
        for j in range(start_lon - x, start_lon + x + 1):
          for k in range(end_lat - x, end_lat + x + 1):
            for m in range(end_lon - x, end_lon + x + 1):
              route_counts[((i,j), (k,m))] += 1

    percentages = sorted([(count/total_rides*100) for count in route_counts.values()], reverse=True)
    route_results[x] = percentages

  return route_results

def plot_percentages(results, x_values, location_type):
  """Visualization with enhanced y-axis gridlines"""
  plt.figure(figsize=(14, 8))

  for x in x_values:
    percentages = results[x]
    plt.plot(np.arange(len(percentages)) + 1,
             percentages,
             label=f'x={x} ({(2*x+1)}x{(2*x+1)} neighborhood)',
             alpha=0.7,
             linewidth=2)

  plt.xlabel('Grid Location Rank (by popularity)', fontsize=12)
  plt.ylabel('% of Total Rides', fontsize=12)
  plt.title(f'{location_type.capitalize()} Location Ride Concentration', fontsize=14)

  # Enhanced grid and ticks
  plt.xscale('log')

  # Calculate y-axis ticks - major and minor
  max_pct = max(max(percentages) for percentages in results.values())
  y_max = max_pct * 1.1

  # Major ticks every 1% (or appropriate interval)
  major_interval = max(0.5, round(y_max/10, 1))  # At least 0.5% interval
  major_ticks = np.arange(0, y_max + major_interval, major_interval)

  # Minor ticks at 0.1% intervals
  minor_ticks = np.arange(0, y_max + 0.1, 0.1)

  plt.yticks(major_ticks, [f"{y:.1f}%" for y in major_ticks], fontsize=10)
  plt.minorticks_on()
  plt.grid(which='major', linestyle='-', alpha=0.7)
  plt.grid(which='minor', linestyle=':', alpha=0.4)

  # Custom x-axis ticks
  plt.xticks([1, 10, 100, 1000, 10000],
             ['1', '10', '100', '1,000', '10,000'],
             fontsize=10)

  plt.legend(fontsize=12)
  plt.tight_layout()
  plt.savefig(f"{OUTPUT_DIR}/{location_type}_ride_percentage.png",
              dpi=300, bbox_inches='tight')
  plt.close()

def plot_route_percentages(route_results, x_values):
  """Route visualization with enhanced y-axis"""
  plt.figure(figsize=(14, 8))

  for x in x_values:
    percentages = route_results[x]
    plt.plot(np.arange(len(percentages)) + 1,
             percentages,
             label=f'x={x} ({(2*x+1)}x{(2*x+1)} neighborhoods)',
             alpha=0.7,
             linewidth=2)

  plt.xlabel('Route Rank (by popularity)', fontsize=12)
  plt.ylabel('% of Total Rides', fontsize=12)
  plt.title('Popular Ride Routes in Austin', fontsize=14)

  # Enhanced grid and ticks
  plt.xscale('log')

  # Y-axis setup
  max_pct = max(max(percentages) for percentages in route_results.values())
  y_max = max_pct * 1.1

  # Major ticks
  major_interval = max(0.1, round(y_max/10, 2))  # At least 0.1% interval
  major_ticks = np.arange(0, y_max + major_interval, major_interval)

  # Minor ticks at 0.02% intervals
  minor_ticks = np.arange(0, y_max + 0.02, 0.02)

  plt.yticks(major_ticks, [f"{y:.2f}%" for y in major_ticks], fontsize=10)
  plt.minorticks_on()
  plt.grid(which='major', linestyle='-', alpha=0.7)
  plt.grid(which='minor', linestyle=':', alpha=0.4)

  # X-axis
  plt.xticks([1, 10, 100, 1000],
             ['1', '10', '100', '1,000'],
             fontsize=10)

  plt.legend(fontsize=12)
  plt.tight_layout()
  plt.savefig(f"{OUTPUT_DIR}/route_percentage.png",
              dpi=300, bbox_inches='tight')
  plt.close()

def print_results(results, x_values, location_type):
  """Improved results printing with more statistics"""
  for x in x_values:
    percentages = results[x]
    print(f"\n{location_type.capitalize()} Locations - Neighborhood size x={x}:")
    print("="*60)
    print(f"{'Rank':<5}{'Percentage':<12}{'Cumulative %':<15}")
    print("-"*32)

    cum_percentage = 0.0
    for i, pct in enumerate(percentages[:20]):
      cum_percentage += pct
      print(f"{i+1:<5}{pct:.6f}%{'':<5}{cum_percentage:.2f}%")

    print("\nSummary statistics:")
    print(f"- Coverage by top 10 locations: {sum(percentages[:10]):.2f}%")
    print(f"- Coverage by top 100 locations: {sum(percentages[:100]):.2f}%")
    print(f"- Locations covering >0.1% of rides: {sum(1 for p in percentages if p > 0.1)}")
    print(f"- Locations covering >1% of rides: {sum(1 for p in percentages if p > 1)}")


def sample_days(df, days, output_path):
  """Sample specific dates from the dataframe"""
  target_dates = [pd.to_datetime(day).date() for day in days]
  day_data = df[df['started_on'].dt.date.isin(target_dates)]
  day_data.to_csv(output_path, index=False)
  print(f"Saved {len(day_data)} rides from {len(target_dates)} days to {output_path}")

def get_busiest_week(df):
  """Return the Monday-Sunday period with most rides"""
  df_weekly = df.resample('W-Mon', on='started_on').size()
  busiest_week_start = df_weekly.idxmax()
  return pd.date_range(busiest_week_start, periods=7).date.tolist()

def get_busiest_month(df):
  """Return all dates from the month with most rides"""
  df_monthly = df.resample('M', on='started_on').size()
  busiest_month = df_monthly.idxmax()
  return pd.date_range(busiest_month, periods=busiest_month.days_in_month).date.tolist()

def main():
  df = load_and_clean_data()
  busiest_date = df['started_on'].dt.date.value_counts().idxmax()
  sample_days(df, [busiest_date.strftime('%Y-%m-%d')], SAMPLE_DAY_CSV)

  busiest_week_dates = get_busiest_week(df)
  sample_days(df, [d.strftime('%Y-%m-%d') for d in busiest_week_dates], SAMPLE_WEEK_CSV)

  busiest_month_dates = get_busiest_month(df)
  sample_days(df, [d.strftime('%Y-%m-%d') for d in busiest_month_dates], SAMPLE_MONTH_CSV)

  df['date'] = df['started_on'].dt.date
  df['day_of_week'] = df['started_on'].dt.day_name()
  df['hour'] = df['started_on'].dt.hour

  # plot_daily_rides(df)
  # plot_hourly_patterns(df)
  plot_austin_heatmap(df, False)

  print("\nData Summary:")
  print(f"Time period: {df['started_on'].min().date()} to {df['started_on'].max().date()}")
  print(f"Total rides after cleaning: {len(df):,}")
  print(f"Average rides per day: {len(df)/df['date'].nunique():.1f}")


  samp = pd.read_csv(SAMPLE_DAY_CSV)
  x_values = [1, 2 , 3]

  # start_results = calculate_location_percentages(samp, x_values, location_type='start')
  # plot_percentages(start_results, x_values, 'start')
  #
  # end_results = calculate_location_percentages(samp, x_values, location_type='end')
  # plot_percentages(end_results, x_values, 'end')

  # route_results = calculate_route_percentages(samp, x_values)
  # plot_route_percentages(route_results, x_values)

  # print_results(start_results, x_values, 'start')
  # print_results(end_results, x_values, 'end')
  # print_results(route_results, x_values, 'route')

if __name__ == "__main__":
  main()